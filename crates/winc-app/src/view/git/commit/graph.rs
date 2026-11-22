//! 提交视图底部的提交图：标题行和最近若干提交，每行是车道、提交说明和作者。
//!
//! 与 Git 日志标签画的是同一张图（车道算法见 [`winc_core::graph`]，线条画法见
//! [`crate::view::git::log::graph`]）：最左边几列画车道，说明从车道右边开始，作者淡色排在行尾。
//! 一行一个提交，点一行打开该提交（见 [`App::press_graph_row`]）；行数多于可见高度时用滚轮滚动。
//!
//! 标题行右端是获取、拉取、推送三个按钮和对比控件：对比控件写着基准的名字和领先、落后的个
//! 数（默认与上游比，见 [`App::graph_compare_base`]），点它换基准。对比进行时，基准缺少的提
//! 交在说明前标一个向上的箭头，基准独有的提交标一个向下的箭头；本地独有的提交（还没推送）
//! 圆点和连着的线涂新增色，基准独有的（还没拉下来）涂强调色，三种标记同一组颜色，一条线上
//! 哪段只在本地、哪段在线上一眼可辨；行尾有引用的行画日志页同款的引用徽章，这时不画作者。

use winc_editor::git::Ref;

use super::*;
use crate::app::git::graph::{GRAPH_GRIP, GRAPH_HEADER, GRAPH_ROW};
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
    // 上沿可以按住上下拉，调整图占的高度：分隔线一像素，光标停在附近也算。悬停、拖动时把分隔线
    // 描成强调色，看得出这里能拉。
    let pulling =
        app.pointer.hover == Hover::Commit(CommitTarget::GraphDivider) || app.graph_resizing();
    if pulling {
        frame.rect_clipped(Rect::new(area.x, area.y, area.w, 1.0), p.accent, 0.0, clip);
    }
    hits.push((
        Rect::new(area.x, area.y, area.w, GRAPH_GRIP),
        CommitTarget::GraphDivider,
    ));
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
    // 悬停或已展开某一行时高亮该提交所在的整条分支线，其余无关车道柔和淡化；
    // 无聚焦时所有车道保持清晰一致，避免竖线逐行明暗闪烁。
    let active_line = match app.pointer.hover {
        Hover::Commit(
            CommitTarget::GraphRow(idx)
            | CommitTarget::GraphOpenLog(idx)
            | CommitTarget::GraphCopyOid(idx),
        ) => graph.rows.get(idx).map(|r| r.line),
        _ => graph.expanded.as_deref().and_then(|oid| {
            graph
                .commits
                .iter()
                .position(|c| c.oid == oid)
                .and_then(|idx| graph.rows.get(idx))
                .map(|r| r.line)
        }),
    };
    // 一行一行往下排：提交行占一行的高度，展开的提交下面跟着它改动的文件。
    let mut top = rows_area.y - scroll;
    for (i, (commit, row)) in graph.commits.iter().zip(&graph.rows).enumerate() {
        let visible = top + GRAPH_ROW > rows_area.y && top < rows_area.y + shown;
        let expanded = graph.expanded.as_deref() == Some(commit.oid.as_str());
        let side = graph.compare.as_ref().and_then(|c| c.side(&commit.oid));
        if visible {
            let row_rect = Rect::new(area.x, top, area.w, GRAPH_ROW).intersect(rows_area);
            let row_hovered = matches!(
                app.pointer.hover,
                Hover::Commit(
                    CommitTarget::GraphRow(idx)
                        | CommitTarget::GraphOpenLog(idx)
                        | CommitTarget::GraphCopyOid(idx)
                ) if idx == i
            );
            let fill = if row_hovered {
                Some(p.hover)
            } else if expanded {
                Some(p.selected_unfocused)
            } else {
                None
            };
            if let Some(fill) = fill {
                let band = Rect::new(area.x + 6.0, top + 1.0, area.w - 12.0, GRAPH_ROW - 2.0);
                frame.rect_clipped(band.intersect(rows_area), fill, RADIUS_ROW, clip);
            }
            let mid = top + GRAPH_ROW / 2.0;
            let ink = |color: u8| lane_color(p, color);
            // 与基准对比时，本地独有的提交（还没推送）用新增色、基准独有的（还没拉下来）用
            // 强调色，与标题行对比控件的个数、说明前的箭头同一组颜色：一条线上哪段只在本地、
            // 哪段在线上一眼可辨。
            let state_ink = |ahead: bool| if ahead { p.git_added } else { p.accent };
            // 上半行、下半行的线。淡色与行底色混好再画，圆头不会在行的交界处叠出深色。
            let bg = fill.map_or(p.editor_bg, |c| {
                crate::view::workspace::panel::over(p.editor_bg, c)
            });
            for (segments, above) in [(&row.up, true), (&row.down, false)] {
                for s in segments {
                    // 连着圆点的那几段属于这一行的提交，随它着色；别的线只是路过，保持车道色。
                    let touches = if above {
                        s.to == row.lane
                    } else {
                        s.from == row.lane
                    };
                    let base = match side.filter(|_| touches) {
                        Some(ahead) => state_ink(ahead),
                        None => ink(s.color),
                    };
                    let faded = active_line.is_some_and(|l| l != s.line);
                    let color = if faded {
                        crate::view::workspace::panel::over(bg, base.alpha(base.a * GRAPH_FADED))
                    } else {
                        base
                    };
                    let stroke_w = if active_line == Some(s.line) {
                        GRAPH_LINE + 0.4
                    } else {
                        GRAPH_LINE
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
                    frame.stroke(&edge, stroke_w, color, clip);
                }
            }
            // 圆点：`HEAD` 是空心圆，合并提交是外环加内芯的双层节点；悬停或已展开的提交外围带一层柔和光晕。
            let cx = x0 + row.lane.min(MAX_LANES - 1) as f32 * LANE + LANE / 2.0;
            let head_commit = commit
                .refs
                .iter()
                .any(|r| matches!(r, Ref::Head(_) | Ref::Detached));
            let is_merge = commit.parents.len() > 1;
            let r = if head_commit {
                4.0
            } else if is_merge {
                3.75
            } else {
                3.25
            };
            let node_ink = side.map_or_else(|| ink(row.color), state_ink);
            if row_hovered || expanded {
                let halo = r + 2.25;
                frame.rect_clipped(
                    Rect::new(cx - halo, mid - halo, 2.0 * halo, 2.0 * halo),
                    node_ink.alpha(0.22),
                    halo,
                    clip,
                );
            }
            // 节点外圈加一层与行底色相同的隔离环，与纵横车道线清晰分离。
            let sep = r + 1.1;
            frame.rect_clipped(
                Rect::new(cx - sep, mid - sep, 2.0 * sep, 2.0 * sep),
                bg,
                sep,
                clip,
            );
            let dot = Rect::new(cx - r, mid - r, 2.0 * r, 2.0 * r);
            frame.rect_clipped(dot, node_ink, r, clip);
            if head_commit {
                let hole = r - GRAPH_LINE;
                frame.rect_clipped(
                    Rect::new(cx - hole, mid - hole, 2.0 * hole, 2.0 * hole),
                    bg,
                    hole,
                    clip,
                );
            } else if is_merge {
                let hole = r - 1.35;
                frame.rect_clipped(
                    Rect::new(cx - hole, mid - hole, 2.0 * hole, 2.0 * hole),
                    bg,
                    hole,
                    clip,
                );
                let core = 1.15;
                frame.rect_clipped(
                    Rect::new(cx - core, mid - core, 2.0 * core, 2.0 * core),
                    node_ink,
                    core,
                    clip,
                );
            }
            // 领先（基准缺少的提交）标向上的箭头，落后（基准独有的提交）标向下的箭头。
            if let Some(room) = mark {
                let center = (text_x - room / 2.0, mid);
                match side {
                    Some(ahead) if ahead => icons::push_small(frame, center, p.git_added, clip),
                    Some(_) => icons::pull_small(frame, center, p.accent, clip),
                    None => {}
                }
            }
            // 说明从标记列右边开始；悬停时右端浮现「复制提交号」「在日志中查看」快捷按钮，
            // 平时有引用时画徽章，否则作者淡色排在行尾。
            let right = row_rect.right() - 8.0;
            let subject_in = &commit.subject;
            let subject_color = if is_merge { p.text_muted } else { p.text };
            if row_hovered {
                let btn_s = 18.0;
                let gap = 2.0;
                let btn_y = (top + (GRAPH_ROW - btn_s) / 2.0).round();
                let log_btn = Rect::new(right - btn_s, btn_y, btn_s, btn_s);
                let copy_btn = Rect::new(log_btn.x - gap - btn_s, btn_y, btn_s, btn_s);
                for (rect, target) in [
                    (copy_btn, CommitTarget::GraphCopyOid(i)),
                    (log_btn, CommitTarget::GraphOpenLog(i)),
                ] {
                    let on = app.pointer.hover == Hover::Commit(target);
                    draw_button(frame, p, rect, clip, on, false);
                    let btn_ink = if on { p.text } else { p.text_muted };
                    match target {
                        CommitTarget::GraphCopyOid(_) => {
                            icons::copy(frame, center(rect), btn_ink, clip);
                        }
                        _ => {
                            icons::history(frame, center(rect), btn_ink, clip);
                        }
                    }
                    hits.push((rect.intersect(rows_area), target));
                }
                let subject_room = (copy_btn.x - 6.0 - text_x).max(0.0);
                let subject = truncate_end(frame, subject_in, size, subject_room);
                label(
                    frame,
                    app,
                    size,
                    (text_x, row_rect),
                    &subject,
                    subject_color,
                    clip,
                );
            } else {
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
                label(
                    frame,
                    app,
                    size,
                    (text_x, row_rect),
                    &subject,
                    subject_color,
                    clip,
                );
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
            }
            hits.push((row_rect, CommitTarget::GraphRow(i)));
        }
        top += GRAPH_ROW;
        // 展开的提交：下面就地列出它改动的文件，同时在左侧继续绘制穿过这些行的车道竖线，
        // 保持提交图连贯不断裂。
        if expanded {
            top = draw_graph_files(
                app,
                frame,
                (top, x0, text_x),
                (row, active_line, side),
                rows_area,
                clip,
                hits,
            );
        }
    }
    // 最后一行下面的空白处也能按住拖动；排在行后面，行本身仍然优先。
    if rows_area.h > 0.0 {
        hits.push((rows_area, CommitTarget::GraphBlank));
    }
}

/// 展开的提交下面列出它改动的文件：每行是状态字母（按变化的种类着色）和路径，点击直接打开
/// 工作区里的当前内容；改了名的按「旧名 → 新名」写。文件还在读取时是一行淡色的提示。
///
/// 文件行从 `text_x` 开始（提交的说明那一列，比车道往右缩进），不参与拖动滚动：点住即可打开。
/// 返回画完后的 y（下一行提交的上沿）。
#[allow(clippy::too_many_arguments)]
fn draw_graph_files(
    app: &App,
    frame: &mut Frame<'_>,
    (top, x0, text_x): (f32, f32, f32),
    (row, active_line, side): (&winc_core::graph::GraphRow, Option<u32>, Option<bool>),
    rows_area: Rect,
    clip: Rect,
    hits: &mut Vec<(Rect, CommitTarget)>,
) -> f32 {
    let graph = &app.git.graph;
    let p = app.palette;
    let size = app.fonts.ui_small;
    let state_ink = |ahead: bool| if ahead { p.git_added } else { p.accent };
    // 在展开文件行的左侧续画从该提交向下延伸的各条车道竖线，保持整张提交图不断裂。
    let draw_lanes = |frame: &mut Frame<'_>, row_y: f32, bg: Color| {
        for s in &row.down {
            let touches = s.from == row.lane;
            let base = match side.filter(|_| touches) {
                Some(ahead) => state_ink(ahead),
                None => lane_color(p, s.color),
            };
            let faded = active_line.is_some_and(|l| l != s.line);
            let color = if faded {
                crate::view::workspace::panel::over(bg, base.alpha(base.a * GRAPH_FADED))
            } else {
                base
            };
            let stroke_w = if active_line == Some(s.line) {
                GRAPH_LINE + 0.4
            } else {
                GRAPH_LINE
            };
            let lx = x0 + s.to.min(MAX_LANES - 1) as f32 * LANE + LANE / 2.0;
            frame.stroke(
                &[winc_render::Stroke::Line(lx, row_y, lx, row_y + GRAPH_ROW)],
                stroke_w,
                color,
                clip,
            );
        }
    };
    let mut y = top;
    for (k, file) in graph.files.iter().enumerate() {
        if y + GRAPH_ROW > rows_area.y && y < rows_area.y + rows_area.h {
            let row_rect = Rect::new(rows_area.x, y, rows_area.w, GRAPH_ROW).intersect(rows_area);
            let hovered = matches!(
                app.pointer.hover,
                Hover::Commit(CommitTarget::GraphFile(i) | CommitTarget::GraphFileDiff(i)) if i == k
            );
            let bg = if hovered {
                let band = Rect::new(
                    rows_area.x + 6.0,
                    y + 1.0,
                    rows_area.w - 12.0,
                    GRAPH_ROW - 2.0,
                );
                frame.rect_clipped(band.intersect(rows_area), p.hover, RADIUS_ROW, clip);
                crate::view::workspace::panel::over(p.editor_bg, p.hover)
            } else {
                p.editor_bg
            };
            draw_lanes(frame, y, bg);
            // 文件列表左侧一条柔和的竖向引导线，与提交行对齐，区分嵌套的文件行。
            let guide_x = (text_x - 5.0).round();
            let guide_color = if hovered {
                p.indent_guide_active
            } else {
                p.indent_guide
            };
            frame.rect_clipped(Rect::new(guide_x, y, 1.0, GRAPH_ROW), guide_color, 0.0, clip);
            let right = row_rect.right() - 8.0;
            let letter = file.change.letter().to_string();
            let letter_w = frame.measure(FontKind::Ui, size, &letter);
            let ink = crate::view::workspace::sidebar::git_color(p, file.change);
            let badge_w = 16.0;
            let badge_h = 15.0;
            let badge_rect = Rect::new(
                right - badge_w,
                (y + (GRAPH_ROW - badge_h) / 2.0).round(),
                badge_w,
                badge_h,
            );
            let pill_fill = ink.alpha(ink.a * if p.dark { 0.16 } else { 0.12 });
            frame.rect_clipped(badge_rect, pill_fill, 4.0, clip);
            label(
                frame,
                app,
                size,
                (badge_rect.x + (badge_w - letter_w) / 2.0, row_rect),
                &letter,
                ink,
                clip,
            );

            // 悬停时在状态徽章左侧浮现「查看差异」快捷按钮，点击直接在编辑区打开该提交下该文件的差异。
            let diff_btn = Rect::new(
                badge_rect.x - 4.0 - 18.0,
                (y + (GRAPH_ROW - 18.0) / 2.0).round(),
                18.0,
                18.0,
            );
            let text_right = if hovered {
                let on_diff = app.pointer.hover == Hover::Commit(CommitTarget::GraphFileDiff(k));
                draw_button(frame, p, diff_btn, clip, on_diff, false);
                let btn_ink = if on_diff { p.text } else { p.text_muted };
                icons::compare(frame, center(diff_btn), btn_ink, clip);
                hits.push((diff_btn.intersect(rows_area), CommitTarget::GraphFileDiff(k)));
                diff_btn.x - 4.0
            } else {
                badge_rect.x - 6.0
            };

            let file_name = winc_editor::display_name(&file.path).into_owned();
            let cy = icon_center_y(app, row_rect, size);
            let icon_end = draw_file_icon(frame, p, (text_x, cy), &file_name, clip);
            let name_x = icon_end + 6.0;
            let room = (text_right - name_x).max(0.0);
            let name_color = if file.change == Change::Deleted {
                p.text_muted
            } else {
                p.text
            };
            if file.from.is_some() {
                let text = app.graph_file_label(k).unwrap_or_default();
                let text = truncate_end(frame, &text, size, room);
                label(frame, app, size, (name_x, row_rect), &text, name_color, clip);
            } else {
                let shown_name = truncate_end(frame, &file_name, size, room);
                let name_end = label(
                    frame,
                    app,
                    size,
                    (name_x, row_rect),
                    &shown_name,
                    name_color,
                    clip,
                );
                let rel_full = app.graph_file_label(k).unwrap_or_default();
                let dir_part = rel_full
                    .strip_suffix(file_name.as_str())
                    .map(|d| d.trim_end_matches(['/', '\\']))
                    .unwrap_or_default();
                let dir_room = text_right - name_end - 6.0;
                if !dir_part.is_empty() && dir_room > 20.0 {
                    let shown_dir = truncate_end(frame, dir_part, size, dir_room);
                    label(
                        frame,
                        app,
                        size,
                        (name_end + 6.0, row_rect),
                        &shown_dir,
                        p.text_dim,
                        clip,
                    );
                }
            }
            hits.push((row_rect, CommitTarget::GraphFile(k)));
        }
        y += GRAPH_ROW;
    }
    // 还在读取：先留出一行淡色的提示，高度与之后画出的文件行一致，滚动不跳。
    if graph.files_loading {
        if y + GRAPH_ROW > rows_area.y && y < rows_area.y + rows_area.h {
            draw_lanes(frame, y, p.editor_bg);
            let rect = Rect::new(rows_area.x, y, rows_area.w, GRAPH_ROW).intersect(rows_area);
            label(
                frame,
                app,
                size,
                (text_x, rect),
                t!("git-graph-files-loading"),
                p.text_dim,
                clip,
            );
        }
        y += GRAPH_ROW;
    }
    y
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
