//! 提交视图底部的提交图：1:1 对齐 VS Code Source Control Graph 的视觉与交互。
//!
//! - 顶部标题行：左侧可折叠的「v 图表」，右侧依次为对比基准芯片（`⎇ 自动`）、聚焦 HEAD（`◎`）、
//!   获取（Fetch）、拉取（Pull）、推送（Push）、刷新（`↻`）、打开完整历史（`⋯`）。
//! - 提交行：左侧连续清脆的车道线与节点（HEAD 为空心蓝环 `○`、普通提交为实心蓝点 `●`、合并提交
//!   为双环），中间为提交说明紧跟淡色作者名（`subject  author`），右侧为 VS Code 风格胶囊徽章
//!   （如蓝底 `[◎ master]` 与紫色云朵 `[☁]`）。
//! - 展开提交后就地列出改动的文件：左侧续画连续车道线与竖向缩进参考线，每行显示彩色文件图标、
//!   文件名、淡色相对目录与右侧彩色状态字母（`M`/`A`/`D`/`R`）；点击文件行直接打开该文件在该
//!   提交中的完整内容差异（同时保持侧栏与提交图可见），悬停时在状态字母左侧浮现「打开当前文件」
//!   快捷图标。

use winc_editor::git::Ref;

use super::*;
use crate::app::git::graph::{GRAPH_GRIP, GRAPH_HEADER, GRAPH_ROW};
use crate::view::git::log::badges::{badges, badges_width, draw_badges, fit_badges};
use crate::view::git::log::graph::lane_edge;
use crate::view::git::log::{
    BADGE_GAP, BADGE_MIN, GRAPH_FADED, GRAPH_LINE, LANE, MAX_LANES, lane_color,
};

/// 提交图内联作者名与提交说明之间的间隔（逻辑像素）。
const AUTHOR_INLINE_GAP: f32 = 6.0;
/// 提交图左边到第一条车道的距离（逻辑像素）。
const GRAPH_PAD: f32 = 10.0;
/// 对比进行时说明左边留给领先、落后标记的宽度（逻辑像素）：每行都一样宽，说明不随行跳动。
const MARK_ROOM: f32 = 13.0;
/// 说明至少留这么宽，放不下徽章时不画徽章（逻辑像素）。
const SUBJECT_MIN: f32 = 64.0;
/// 行尾的引用徽章最多占这么宽（逻辑像素）。
const BADGE_ROOM: f32 = 138.0;
/// 标题行右端工具按钮的边长（逻辑像素）：紧凑对齐 VS Code 图表标题栏。
const TOOL: f32 = 18.0;
/// 按钮之间的间隔（逻辑像素）。
const TOOL_GAP: f32 = 1.0;
/// 对比控件里基准名最多占的宽度（逻辑像素）。
const COMPARE_NAME_MAX: f32 = 68.0;
/// 对比控件里基准名至少留这么宽（逻辑像素），更窄时不画控件，把位置让给右边的按钮。
const NAME_MIN: f32 = 12.0;
/// 对比控件里内容和左右留白的间隔（逻辑像素）。
const COMPARE_PAD: f32 = 4.0;

/// 绘制提交图：上沿一条分隔线，一行可折叠标题与工具栏，下面每行一个提交。
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
    let graph = &app.git.graph;
    frame.rect_clipped(
        Rect::new(area.x, area.y, area.w, 1.0),
        p.separator,
        0.0,
        clip,
    );
    // 展开时上沿可以按住上下拉，调整图占的高度：分隔线一像素，光标停在附近也算。悬停、拖动时
    // 描成强调色。
    if !graph.collapsed {
        let pulling =
            app.pointer.hover == Hover::Commit(CommitTarget::GraphDivider) || app.graph_resizing();
        if pulling {
            frame.rect_clipped(Rect::new(area.x, area.y, area.w, 1.0), p.accent, 0.0, clip);
        }
        hits.push((
            Rect::new(area.x, area.y, area.w, GRAPH_GRIP),
            CommitTarget::GraphDivider,
        ));
    }

    let size = app.fonts.ui_small;
    let head = Rect::new(area.x, area.y, area.w, GRAPH_HEADER);
    // 左侧可折叠的「v 图表」标题，与 VS Code 侧栏折叠节标题保持一致。
    let toggle_hovered = app.pointer.hover == Hover::Commit(CommitTarget::GraphToggle);
    let chevron_x = area.x + 12.0;
    let head_mid = head.y + head.h / 2.0;
    let angle = if graph.collapsed { 0.0 } else { FRAC_PI_2 };
    icons::chevron_turned(
        frame,
        (chevron_x, head_mid),
        6.0,
        angle,
        if toggle_hovered { p.text } else { p.text_muted },
        clip,
    );
    let title = t!("git-graph-title");
    let title_x = area.x + 22.0;
    label(
        frame,
        app,
        size,
        (title_x, head),
        title,
        if toggle_hovered { p.text } else { p.text_muted },
        clip,
    );
    let title_right = title_x + frame.measure(FontKind::Ui, size, title);
    let toggle_top = if graph.collapsed {
        head.y
    } else {
        head.y + GRAPH_GRIP
    };
    hits.push((
        Rect::new(
            area.x,
            toggle_top,
            (title_right - area.x + 6.0).max(0.0),
            (head.bottom() - toggle_top).max(0.0),
        ),
        CommitTarget::GraphToggle,
    ));

    // 标题行右端工具栏：聚焦 HEAD、获取、拉取、推送、刷新、更多（打开完整历史），其左为对比芯片。
    let buttons_left = draw_tools(app, frame, head, clip, hits);
    if let Some(error) = &graph.error {
        let room = (buttons_left - COMPARE_PAD - title_right - 4.0).max(0.0);
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

    if graph.collapsed || graph.is_empty() {
        return;
    }

    let rows_area = Rect::new(
        area.x,
        head.bottom(),
        area.w,
        (area.h - GRAPH_HEADER).max(0.0),
    );
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
    // 悬停或已展开某一行时高亮该提交所在的整条分支线，其余无关车道柔和淡化。
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
    let base_bg = p.sidebar_bg;
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
            let state_ink = |ahead: bool| if ahead { p.git_added } else { p.accent };
            let bg = fill.map_or(base_bg, |c| crate::view::workspace::panel::over(base_bg, c));
            for (segments, above) in [(&row.up, true), (&row.down, false)] {
                for s in segments {
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
                        GRAPH_LINE + 0.5
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
            // 圆点（1:1 VS Code 风格）：`HEAD` 为醒目空心圆环 `○`，普通提交为清脆实心圆点 `●`，
            // 合并提交为双环。不画过宽的暗色隔离环，让节点与连续竖线自然衔接。
            let cx = x0 + row.lane.min(MAX_LANES - 1) as f32 * LANE + LANE / 2.0;
            let head_commit = commit
                .refs
                .iter()
                .any(|r| matches!(r, Ref::Head(_) | Ref::Detached));
            let is_merge = commit.parents.len() > 1;
            let r = if head_commit {
                4.2
            } else if is_merge {
                3.8
            } else {
                3.4
            };
            let node_ink = side.map_or_else(|| ink(row.color), state_ink);
            if row_hovered || expanded {
                let halo = r + 2.0;
                frame.rect_clipped(
                    Rect::new(cx - halo, mid - halo, 2.0 * halo, 2.0 * halo),
                    node_ink.alpha(0.20),
                    halo,
                    clip,
                );
            }
            let dot = Rect::new(cx - r, mid - r, 2.0 * r, 2.0 * r);
            frame.rect_clipped(dot, node_ink, r, clip);
            if head_commit {
                let hole = (r - GRAPH_LINE).max(1.5);
                frame.rect_clipped(
                    Rect::new(cx - hole, mid - hole, 2.0 * hole, 2.0 * hole),
                    bg,
                    hole,
                    clip,
                );
            } else if is_merge {
                let hole = r - 1.4;
                frame.rect_clipped(
                    Rect::new(cx - hole, mid - hole, 2.0 * hole, 2.0 * hole),
                    bg,
                    hole,
                    clip,
                );
                let core = 1.2;
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
            // 右端：悬停时显示「复制提交号」「在日志中查看」快捷按钮；平时有引用时显示 VS Code 胶囊
            // 徽章（如 `[◎ master] [☁]`）。说明后紧跟淡色作者名（`subject  author`），空间不足时依次截短。
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
                let content_right = copy_btn.x - 6.0;
                draw_subject_and_author(
                    app,
                    frame,
                    (text_x, content_right, row_rect),
                    (subject_in, &commit.author),
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
                let content_right = match &drawn {
                    Some((_, width)) => (right - width - BADGE_GAP).max(text_x),
                    None => right,
                };
                draw_subject_and_author(
                    app,
                    frame,
                    (text_x, content_right, row_rect),
                    (subject_in, &commit.author),
                    subject_color,
                    clip,
                );
                if let Some((badges, width)) = drawn {
                    let left = right - width;
                    draw_badges(app, frame, (left, row_rect), &badges, clip);
                }
            }
            hits.push((row_rect, CommitTarget::GraphRow(i)));
        }
        top += GRAPH_ROW;
        // 展开的提交：下面就地列出它改动的文件，同时在左侧续画穿过这些行的车道竖线。
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

/// 在 `text_x..content_right` 内按 VS Code 风格绘制「提交说明 紧跟 淡色作者名」。
///
/// 提交说明优先，如果说明画完后仍有余量，在其后间隔 [`AUTHOR_INLINE_GAP`] 绘制淡色作者名。
fn draw_subject_and_author(
    app: &App,
    frame: &mut Frame<'_>,
    (text_x, content_right, row_rect): (f32, f32, Rect),
    (subject, author): (&str, &str),
    subject_color: Color,
    clip: Rect,
) {
    let size = app.fonts.ui_small;
    let room = (content_right - text_x).max(0.0);
    if room <= 0.0 {
        return;
    }
    let shown_subject = truncate_end(frame, subject, size, room);
    let subject_end = label(
        frame,
        app,
        size,
        (text_x, row_rect),
        &shown_subject,
        subject_color,
        clip,
    );
    // 说明本身已截短或没有作者时不画内联作者。
    if shown_subject != subject || author.is_empty() {
        return;
    }
    let author_x = subject_end + AUTHOR_INLINE_GAP;
    let author_room = content_right - author_x;
    if author_room > 16.0 {
        let shown_author = truncate_end(frame, author, size, author_room);
        label(
            frame,
            app,
            size,
            (author_x, row_rect),
            &shown_author,
            app.palette.text_dim,
            clip,
        );
    }
}

/// 展开的提交下面列出它改动的文件：点击文件行直接打开该文件在该提交中的完整差异，悬停时在
/// 状态字母左侧显示「打开当前文件」图标按钮，1:1 对齐 VS Code 图表展开文件行。
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
    let base_bg = p.sidebar_bg;
    // 在展开文件行的左侧续画从该提交向下延伸的各条车道竖线，保持整张提交图连贯不断裂。
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
                GRAPH_LINE + 0.6
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
                Hover::Commit(CommitTarget::GraphFile(i) | CommitTarget::GraphFileOpen(i)) if i == k
            );
            let selected = graph.selected_file == Some(k);
            let fill = if hovered {
                Some(p.hover)
            } else if selected {
                Some(p.selected_unfocused)
            } else {
                None
            };
            let bg = if let Some(fill) = fill {
                let band = Rect::new(
                    rows_area.x + 6.0,
                    y + 1.0,
                    rows_area.w - 12.0,
                    GRAPH_ROW - 2.0,
                );
                frame.rect_clipped(band.intersect(rows_area), fill, RADIUS_ROW, clip);
                crate::view::workspace::panel::over(base_bg, fill)
            } else {
                base_bg
            };
            draw_lanes(frame, y, bg);
            // 文件列表左侧一条柔和的竖向引导线，与提交行对齐，区分嵌套的文件行。
            let guide_x = (text_x - 5.0).round();
            let guide_color = if hovered || selected {
                p.indent_guide_active
            } else {
                p.indent_guide
            };
            frame.rect_clipped(
                Rect::new(guide_x, y, 1.0, GRAPH_ROW),
                guide_color,
                0.0,
                clip,
            );
            // 右侧清脆的彩色单字母状态（M / A / D / R），无多余背景框，对齐 VS Code。
            let right = row_rect.right() - 10.0;
            let letter = file.change.letter().to_string();
            let letter_w = frame.measure(FontKind::Ui, size, &letter);
            let ink = crate::view::workspace::sidebar::git_color(p, file.change);
            let status_x = right - letter_w;
            label(frame, app, size, (status_x, row_rect), &letter, ink, clip);

            // 悬停或选中时在状态字母左侧显示「打开文件」图标按钮，点击打开工作区当前文件。
            let open_btn = Rect::new(
                status_x - 6.0 - 18.0,
                (y + (GRAPH_ROW - 18.0) / 2.0).round(),
                18.0,
                18.0,
            );
            let text_right = if hovered || selected {
                let on_open = app.pointer.hover == Hover::Commit(CommitTarget::GraphFileOpen(k));
                draw_button(frame, p, open_btn, clip, on_open, false);
                let btn_ink = if on_open { p.text } else { p.text_muted };
                let (cx, cy) = center(open_btn);
                let (w, h) = (9.0, 11.0);
                icons::document(
                    frame,
                    ((cx - w / 2.0).round(), (cy - h / 2.0).round()),
                    (w, h),
                    btn_ink,
                    clip,
                );
                hits.push((
                    open_btn.intersect(rows_area),
                    CommitTarget::GraphFileOpen(k),
                ));
                open_btn.x - 4.0
            } else {
                status_x - 8.0
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
                label(
                    frame,
                    app,
                    size,
                    (name_x, row_rect),
                    &text,
                    name_color,
                    clip,
                );
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
            draw_lanes(frame, y, base_bg);
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

/// 画标题行右端的工具按钮（从左到右：聚焦 HEAD、获取、拉取、推送、刷新、更多），返回最左边按钮的左边缘。
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
        (CommitTarget::GraphFocusHead, true),
        (CommitTarget::GraphFetch, remote && !busy),
        (CommitTarget::GraphPull, remote && !busy),
        (CommitTarget::GraphPush, remote && !busy),
        (CommitTarget::GraphRefresh, true),
        (CommitTarget::GraphMore, true),
    ];
    let top = head.y + (head.h - TOOL) / 2.0;
    let left =
        head.right() - 6.0 - buttons.len() as f32 * TOOL - (buttons.len() - 1) as f32 * TOOL_GAP;
    for (i, (target, enabled)) in buttons.into_iter().enumerate() {
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
            CommitTarget::GraphFocusHead => icons::target_head(frame, at, color, clip),
            CommitTarget::GraphFetch => icons::graph_fetch(frame, at, color, clip),
            CommitTarget::GraphPull => icons::graph_pull(frame, at, color, clip),
            CommitTarget::GraphPush => icons::graph_push(frame, at, color, clip),
            CommitTarget::GraphRefresh => icons::refresh(frame, at, color, clip),
            _ => icons::more(frame, at, color, clip),
        }
        hits.push((rect, target));
    }
    left
}

/// 画标题行的对比控件（VS Code 风格：`⎇ 自动` 或自定义基准名，有领先/落后时附带简洁计数）。
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
    let has_diff = ahead > 0 || behind > 0;
    let counts = has_diff.then(|| (format!("↑{ahead}"), format!("↓{behind}")));
    let counts_w = counts.as_ref().map_or(0.0, |(up, down)| {
        let mut w = 0.0;
        if ahead > 0 {
            w += 3.0 + frame.measure(FontKind::Ui, size, up);
        }
        if behind > 0 {
            w += 3.0 + frame.measure(FontKind::Ui, size, down);
        }
        w
    });
    let icon = 11.0 + 3.0;
    let max_room = (right - title_right - 6.0 - icon - 2.0 * COMPARE_PAD).max(0.0);
    let show_counts = counts.is_some() && max_room >= counts_w + NAME_MIN;
    let effective_counts_w = if show_counts { counts_w } else { 0.0 };
    let room = (max_room - effective_counts_w).min(COMPARE_NAME_MAX);
    if room < NAME_MIN {
        return;
    }
    // 自动对比上游时与 VS Code 保持一致显示简洁的「自动」，手动指定其他分支时显示分支名。
    let raw_name = match (&app.git.graph.base, compare) {
        (Some(base), _) => base.as_str(),
        (None, Some(_)) => t!("git-graph-compare-auto-short"),
        (None, None) => t!("git-graph-compare"),
    };
    let name = truncate_end(frame, raw_name, size, room).into_owned();
    let name_w = frame.measure(FontKind::Ui, size, &name);
    let inner = icon + name_w + effective_counts_w;
    let rect = Rect::new(
        right - 3.0 - inner - 2.0 * COMPARE_PAD,
        head.y + (head.h - TOOL) / 2.0,
        inner + 2.0 * COMPARE_PAD,
        TOOL,
    );
    if rect.x < title_right + 4.0 {
        return;
    }
    let hovered = app.pointer.hover == Hover::Commit(CommitTarget::GraphCompare);
    let open = app.overlays.menu_button == Some(MenuButton::GraphCompare);
    draw_button(frame, p, rect, clip, hovered, open);
    let color = if hovered { p.text } else { p.text_muted };
    let cy = rect.y + rect.h / 2.0;
    icons::branch_small(frame, (rect.x + COMPARE_PAD + 5.0, cy), color, clip);
    let mut x = rect.x + COMPARE_PAD + icon;
    x = ui_text_small(frame, app, x, rect, &name, color, clip);
    if show_counts && let Some((up, down)) = counts {
        if ahead > 0 {
            x = ui_text_small(frame, app, x + 3.0, rect, &up, p.git_added, clip);
        }
        if behind > 0 {
            ui_text_small(frame, app, x + 3.0, rect, &down, p.accent, clip);
        }
    }
    hits.push((rect, CommitTarget::GraphCompare));
}
