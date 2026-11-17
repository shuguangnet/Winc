//! 提交视图底部的提交图：标题行和最近若干提交，每行是车道、提交说明和作者。
//!
//! 与 Git 日志标签画的是同一张图（车道算法见 [`winc_core::graph`]，线条画法见
//! [`crate::view::git::log::graph`]）：最左边几列画车道，说明从车道右边开始，作者淡色排在行尾。
//! 一行一个提交，点一行打开该提交（见 [`App::press_graph_row`]）；行数多于可见高度时用滚轮滚动。
//!
//! 标题行右端是获取、拉取、推送三个按钮和对比控件：对比控件写着基准的名字和领先、落后的个
//! 数（默认与上游比，见 [`App::graph_compare_base`]），点它换基准。对比进行时，基准缺少的提
//! 交在说明前标一个向上的箭头，基准独有的提交标一个向下的箭头；行尾有引用的行画日志页同款
//! 的引用徽章，这时不画作者。

use winc_editor::git::Ref;

use super::*;
use crate::app::git::graph::{GRAPH_HEADER, GRAPH_ROW};
use crate::view::git::log::badges::{badges, badges_width, draw_badges, fit_badges};
use crate::view::git::log::graph::lane_edge;
use crate::view::git::log::{
    BADGE_GAP, BADGE_MIN, GRAPH_FADED, GRAPH_LINE, LANE, MAX_LANES, lane_color,
};

/// 提交图一行的作者名字最多占的宽度（逻辑像素）：说明优先，作者只作提示。
const AUTHOR_MAX: f32 = 96.0;
/// 说明与作者之间的间隔（逻辑像素）。
const AUTHOR_GAP: f32 = 8.0;
/// 提交图左边到第一条车道的距离（逻辑像素）。
const GRAPH_PAD: f32 = 10.0;
/// 对比进行时说明左边留给领先、落后标记的宽度（逻辑像素）：每行都一样宽，说明不随行跳动。
const MARK_ROOM: f32 = 13.0;
/// 说明至少留这么宽，放不下徽章时不画徽章（逻辑像素）。
const SUBJECT_MIN: f32 = 72.0;
/// 行尾的引用徽章最多占这么宽（逻辑像素）。
const BADGE_ROOM: f32 = 132.0;
/// 标题行右端三个按钮的边长（逻辑像素）：标题行本身只有 24 高。
const TOOL: f32 = 20.0;
/// 按钮之间的间隔（逻辑像素）。
const TOOL_GAP: f32 = 2.0;
/// 对比控件里基准名最多占的宽度（逻辑像素）。
const COMPARE_NAME_MAX: f32 = 72.0;
/// 对比控件里基准名至少留这么宽（逻辑像素），更窄时不画控件，把位置让给右边的按钮。
const NAME_MIN: f32 = 12.0;
/// 对比控件里内容和左右留白的间隔（逻辑像素）。
const COMPARE_PAD: f32 = 6.0;

/// 绘制提交图：上沿一条分隔线，一行标题，下面每行一个提交。
pub(super) fn draw_graph(
    app: &App,
    frame: &mut Frame<'_>,
    parts: &CommitParts,
    hits: &mut Vec<(Rect, CommitTarget)>,
) {
    let Some(area) = parts.graph else {
        return;
    };
    let p = app.palette;
    let clip = app.layout.sidebar.intersect(area);
    frame.rect_clipped(
        Rect::new(area.x, area.y, area.w, 1.0),
        p.separator,
        0.0,
        clip,
    );
    let size = app.fonts.ui_small;
    let head = Rect::new(area.x, area.y, area.w, GRAPH_HEADER);
    let title = t!("git-graph-title");
    label(
        frame,
        app,
        size,
        (area.x + 8.0, head),
        title,
        p.text_muted,
        clip,
    );
    let title_right = area.x + 8.0 + frame.measure(FontKind::Ui, size, title);
    let rows_area = Rect::new(
        area.x,
        head.bottom(),
        area.w,
        (area.h - GRAPH_HEADER).max(0.0),
    );
    let graph = &app.git.graph;
    // 标题行右端从右到左：推送、拉取、获取，然后是对比控件。
    let buttons_left = draw_tools(app, frame, head, clip, hits);
    if let Some(error) = &graph.error {
        // 读取出错时在按钮左边说明，图里留着上一次的内容。
        let room = (buttons_left - COMPARE_PAD - title_right).max(0.0);
        let text = truncate_end(frame, error, size, room);
        let w = frame.measure(FontKind::Ui, size, &text);
        label(
            frame,
            app,
            size,
            (buttons_left - COMPARE_PAD - w, head),
            &text,
            p.text_dim,
            clip,
        );
    } else {
        draw_compare(app, frame, head, clip, title_right, buttons_left, hits);
    }
    if graph.is_empty() {
        return;
    }
    // 车道最多画 `MAX_LANES` 条，超出的挤在最右边。
    let lanes = graph
        .rows
        .iter()
        .map(|row| row.width)
        .max()
        .unwrap_or(1)
        .min(MAX_LANES);
    let x0 = area.x + GRAPH_PAD;
    // 对比进行时每行都在说明前留出同一列标记，说明的起点不随行移动。
    let mark = graph.compare.as_ref().map(|_| MARK_ROOM);
    let text_x = x0 + lanes as f32 * LANE + 6.0 + mark.unwrap_or(0.0);
    let scroll = app.git.graph.scroll;
    let shown = rows_area.h;
    for (i, (commit, row)) in graph.commits.iter().zip(&graph.rows).enumerate() {
        let top = rows_area.y + i as f32 * GRAPH_ROW - scroll;
        if top + GRAPH_ROW <= rows_area.y || top >= rows_area.y + shown {
            continue;
        }
        let row_rect = Rect::new(area.x, top, area.w, GRAPH_ROW).intersect(rows_area);
        let band = (app.pointer.hover == Hover::Commit(CommitTarget::GraphRow(i)))
            .then(|| Rect::new(area.x + 6.0, top + 1.0, area.w - 12.0, GRAPH_ROW - 2.0));
        if let Some(band) = band {
            frame.rect_clipped(band.intersect(rows_area), p.hover, RADIUS_ROW, clip);
        }
        let mid = top + GRAPH_ROW / 2.0;
        let ink = |color: u8| lane_color(p, color);
        // 上半行、下半行的线。淡色与行底色混好再画，圆头不会在行的交界处叠出深色。
        let bg = if band.is_some() {
            crate::view::workspace::panel::over(p.editor_bg, p.hover)
        } else {
            p.editor_bg
        };
        for (segments, above) in [(&row.up, true), (&row.down, false)] {
            for s in segments {
                let color = ink(s.color);
                let color = if s.line == row.line {
                    color
                } else {
                    crate::view::workspace::panel::over(bg, color.alpha(color.a * GRAPH_FADED))
                };
                let (top_y, bottom_y) = if above {
                    (top, mid)
                } else {
                    (mid, top + GRAPH_ROW)
                };
                let from = (
                    x0 + s.from.min(MAX_LANES - 1) as f32 * LANE + LANE / 2.0,
                    top_y,
                );
                let to = (
                    x0 + s.to.min(MAX_LANES - 1) as f32 * LANE + LANE / 2.0,
                    bottom_y,
                );
                let edge = lane_edge(from, to, above);
                frame.stroke(&edge, GRAPH_LINE, color, clip);
            }
        }
        // 圆点：`HEAD` 是空心圆，合并提交小一圈。
        let cx = x0 + row.lane.min(MAX_LANES - 1) as f32 * LANE + LANE / 2.0;
        let head_commit = commit
            .refs
            .iter()
            .any(|r| matches!(r, Ref::Head(_) | Ref::Detached));
        let r = if head_commit {
            4.0
        } else if commit.parents.len() > 1 {
            2.75
        } else {
            3.25
        };
        let dot = Rect::new(cx - r, mid - r, 2.0 * r, 2.0 * r);
        frame.rect_clipped(dot, ink(row.color), r, clip);
        if head_commit {
            let hole = r - GRAPH_LINE;
            frame.rect_clipped(
                Rect::new(cx - hole, mid - hole, 2.0 * hole, 2.0 * hole),
                if band.is_some() { p.hover } else { p.editor_bg },
                hole,
                clip,
            );
        }
        // 领先（基准缺少的提交）标向上的箭头，落后（基准独有的提交）标向下的箭头。
        if let Some(room) = mark {
            let side = graph.compare.as_ref().and_then(|c| c.side(&commit.oid));
            let center = (text_x - room / 2.0, mid);
            match side {
                Some(ahead) if ahead => icons::push_small(frame, center, p.git_added, clip),
                Some(_) => icons::pull_small(frame, center, p.accent, clip),
                None => {}
            }
        }
        // 说明从标记列右边开始；行尾有引用时画徽章（画得下的话），否则作者淡色排在行尾。
        let right = row_rect.right() - 8.0;
        let subject_in = &commit.subject;
        let badge_room = (right - text_x - SUBJECT_MIN - BADGE_GAP).min(BADGE_ROOM);
        let refs = badges(p, commit);
        let drawn = (!refs.is_empty() && badge_room >= BADGE_MIN)
            .then(|| {
                let fitted = fit_badges(app, frame, &refs, badge_room);
                let width = badges_width(app, frame, &fitted);
                (fitted, width)
            })
            .filter(|(_, width)| *width > 0.0);
        let (subject_room, author) = match &drawn {
            Some((_, width)) => ((right - width - BADGE_GAP - text_x).max(0.0), None),
            None => {
                let author_w = frame
                    .measure(FontKind::Ui, size, &commit.author)
                    .min(AUTHOR_MAX);
                let author_x = (right - author_w).max(text_x + AUTHOR_GAP);
                (
                    (author_x - AUTHOR_GAP - text_x).max(0.0),
                    Some((author_x, author_w)),
                )
            }
        };
        let subject = truncate_end(frame, subject_in, size, subject_room);
        label(frame, app, size, (text_x, row_rect), &subject, p.text, clip);
        if let Some((badges, width)) = drawn {
            let left = right - width;
            draw_badges(app, frame, (left, row_rect), &badges, clip);
        }
        if let Some((author_x, author_w)) = author {
            let author = truncate_end(frame, &commit.author, size, author_w);
            label(
                frame,
                app,
                size,
                (author_x, row_rect),
                &author,
                p.text_dim,
                clip,
            );
        }
        hits.push((row_rect, CommitTarget::GraphRow(i)));
    }
}

/// 画标题行右端的三个按钮（从左到右：获取、拉取、推送），返回最左边那个的左边缘。
///
/// 不能用的（没有远程仓库、正在执行别的 Git 操作）灰掉，原因写在悬停提示里。操作进行中三个
/// 都灰掉：同时开两个会互相打断。
fn draw_tools(
    app: &App,
    frame: &mut Frame<'_>,
    head: Rect,
    clip: Rect,
    hits: &mut Vec<(Rect, CommitTarget)>,
) -> f32 {
    let p = app.palette;
    let busy = app.git.branch.running.is_some();
    let remote = !app.git.remotes.is_empty();
    let buttons = [
        (CommitTarget::GraphFetch, remote),
        (CommitTarget::GraphPull, remote),
        (CommitTarget::GraphPush, remote),
    ];
    let top = head.y + (head.h - TOOL) / 2.0;
    let left = head.right()
        - COMPARE_PAD
        - buttons.len() as f32 * TOOL
        - (buttons.len() - 1) as f32 * TOOL_GAP;
    for (i, (target, enabled)) in buttons.into_iter().enumerate() {
        let enabled = enabled && !busy;
        let x = left + i as f32 * (TOOL + TOOL_GAP);
        let rect = Rect::new(x, top, TOOL, TOOL);
        let hovered = enabled && app.pointer.hover == Hover::Commit(target);
        draw_button(frame, p, rect, clip, hovered, false);
        let color = match (enabled, hovered) {
            (false, _) => p.text_dim,
            (true, true) => p.text,
            (true, false) => p.text_muted,
        };
        let at = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        match target {
            CommitTarget::GraphFetch => icons::sync(frame, at, color, clip),
            CommitTarget::GraphPull => icons::pull_small(frame, at, color, clip),
            _ => icons::push_small(frame, at, color, clip),
        }
        // 不能用的也记下：悬停时写明原因，点了不做事。
        hits.push((rect, target));
    }
    left
}

/// 画标题行的对比控件。
///
/// 内容是基准的名字和领先（↑）、落后（↓）的个数；还没选基准（分支没有上游）时写「对比」。
/// 点它弹出换基准的菜单（[`MenuTarget::GraphCompare`]）。位置不够时整个不画，让给右边的按钮。
fn draw_compare(
    app: &App,
    frame: &mut Frame<'_>,
    head: Rect,
    clip: Rect,
    title_right: f32,
    right: f32,
    hits: &mut Vec<(Rect, CommitTarget)>,
) {
    let p = app.palette;
    let size = app.fonts.ui_small;
    let compare = app.git.graph.compare.as_ref();
    let (ahead, behind) = compare.map_or((0, 0), |c| c.counts);
    let counts = compare.map(|_| (format!("↑{ahead}"), format!("↓{behind}")));
    let counts_w = counts.as_ref().map_or(0.0, |(up, down)| {
        COMPARE_PAD
            + frame.measure(FontKind::Ui, size, up)
            + COMPARE_PAD
            + frame.measure(FontKind::Ui, size, down)
    });
    // 基准名按剩下的位置截短；剩下不到一个字的宽度时不画控件，位置留给右边的按钮。
    let icon = 12.0 + 4.0;
    let room =
        (right - title_right - 8.0 - counts_w - icon - 2.0 * COMPARE_PAD).min(COMPARE_NAME_MAX);
    if room < NAME_MIN {
        return;
    }
    let name = match compare {
        Some(compare) => truncate_end(frame, &compare.base, size, room).into_owned(),
        None => t!("git-graph-compare").to_owned(),
    };
    let name_w = frame.measure(FontKind::Ui, size, &name);
    let inner = icon + name_w + counts_w;
    let rect = Rect::new(
        right - COMPARE_PAD - inner - 2.0 * COMPARE_PAD,
        head.y + (head.h - TOOL) / 2.0,
        inner + 2.0 * COMPARE_PAD,
        TOOL,
    );
    if rect.x < title_right + 8.0 {
        return;
    }
    let hovered = app.pointer.hover == Hover::Commit(CommitTarget::GraphCompare);
    let open = app.overlays.menu_button == Some(MenuButton::GraphCompare);
    draw_button(frame, p, rect, clip, hovered, open);
    let color = if hovered { p.text } else { p.text_muted };
    let center = rect.y + rect.h / 2.0;
    icons::compare(frame, (rect.x + COMPARE_PAD + 6.0, center), color, clip);
    let mut x = rect.x + COMPARE_PAD + 12.0 + 4.0;
    x += ui_text_small(frame, app, x, rect, &name, color, clip);
    if let Some((up, down)) = counts {
        // 领先的个数用新增色、落后的用强调色，与图里行前的标记一致；为 0 时淡掉。
        let dim = |on: bool, color: Color| if on { color } else { p.text_dim };
        x += COMPARE_PAD;
        x += ui_text_small(frame, app, x, rect, &up, dim(ahead > 0, p.git_added), clip);
        x += COMPARE_PAD;
        ui_text_small(frame, app, x, rect, &down, dim(behind > 0, p.accent), clip);
    }
    hits.push((rect, CommitTarget::GraphCompare));
}
