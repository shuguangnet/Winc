//! 文件树行名称溢出与合并目录链的绘制：超宽名称在编辑区上方的延续绘制、收回动画以及
//! 合并目录链的分级折叠与渲染。

use std::borrow::Cow;
use std::path::Path;

use winc_editor::Entry;

use super::*;

/// 返回鼠标所在的文件树行在名称放不下时，于编辑区上方继续绘制的那一段：矩形和该行的行号。
///
/// 不需要接续绘制时返回 `None`。
pub(in crate::view) fn tree_overflow(app: &App, frame: &mut Frame<'_>) -> Option<(Rect, usize)> {
    let index = app.overflow_candidate()?;
    let entry = app.editor.workspace.as_ref()?.rows().get(index)?;
    let tree = app.layout.tree;
    // 固定在顶部的目录在固定的位置继续绘制。
    let row = match app.tree_sticky().into_iter().find(|s| s.index == index) {
        Some(sticky) => sticky.rect,
        None => {
            tree_row_slot(
                tree,
                app.panels.tree.scroll,
                app.tree_slot_of(index),
                None,
                0.0,
            )
            .0
        }
    };
    let shift = -app.panels.tree.scroll_x;
    let end = tree_label_x(row, entry.depth, shift) + label_width(app, frame, entry);

    if !label_overflows(tree, end) {
        return None;
    }

    let right = (end + 8.0).min(app.layout.window.right() - 8.0);
    let rect = Rect::new(tree.right(), row.y + 1.0, right - tree.right(), row.h - 2.0);
    (rect.w > 0.0 && app.mouse().0 < rect.right()).then_some((rect, index))
}

/// 在编辑区上方继续绘制放不下的文件名：底色和文本都与文件树中的那一行连成一体。
pub(in crate::view) fn draw_tree_overflow(app: &App, frame: &mut Frame<'_>) {
    let (Some((rect, index)), Some(ws)) = (app.panels.tree.overflow, &app.editor.workspace) else {
        return;
    };
    let Some(entry) = ws.rows().get(index) else {
        return;
    };
    let view = app.panels.tree.labels.peek(label_key(entry));
    draw_overflow_band(app, frame, (entry, index), rect, view.as_ref(), false);
}

/// 绘制鼠标移开后正在收回的名字：不响应点击，收回到侧栏边缘时消失。
pub(in crate::view) fn draw_tree_overflow_closing(app: &App, frame: &mut Frame<'_>) {
    // 文件树不在显示时不画：动画途中隐藏了侧栏或切换到其他视图。
    if !app.sidebar_open() || app.panels.sidebar.view != SidebarView::Files {
        return;
    }
    let Some(ws) = &app.editor.workspace else {
        return;
    };
    let open = app.panels.tree.overflow.map(|(_, index)| index);
    let tree = app.layout.tree;
    for (key, view) in app.panels.tree.labels.moving_rows() {
        let Some(index) = ws.row_of(&key).filter(|&index| Some(index) != open) else {
            continue;
        };
        let entry = &ws.rows()[index];
        let row = overflow_row(app, index);
        let end = tree_label_x(row, entry.depth, -app.panels.tree.scroll_x) + view.width;
        let right = (end + 8.0).min(app.layout.window.right() - 8.0);
        let rect = Rect::new(tree.right(), row.y + 1.0, right - tree.right(), row.h - 2.0);
        if rect.w > 0.0 {
            draw_overflow_band(app, frame, (entry, index), rect, Some(&view), true);
        }
    }
}

/// 返回第 `index` 行在文件树中的矩形；固定在顶部的目录在固定的位置。
fn overflow_row(app: &App, index: usize) -> Rect {
    let tree = app.layout.tree;
    match app.tree_sticky().into_iter().find(|s| s.index == index) {
        Some(sticky) => sticky.rect,
        None => {
            tree_row_slot(
                tree,
                app.panels.tree.scroll,
                app.tree_slot_of(index),
                None,
                0.0,
            )
            .0
        }
    }
}

/// 在 `rect` 中绘制第 `index` 行超出侧栏的部分，按 `view` 的当前宽度露出。
///
/// `closing` 为真时显示收回途中的旧内容，否则显示这一行的完整名字。
fn draw_overflow_band(
    app: &App,
    frame: &mut Frame<'_>,
    (entry, index): (&Entry, usize),
    rect: Rect,
    view: Option<&LabelView>,
    closing: bool,
) {
    let Some(ws) = &app.editor.workspace else {
        return;
    };
    let p = app.palette;
    let tree = app.layout.tree;
    let row = Rect::new(tree.x, rect.y - 1.0, tree.w, rect.h + 2.0);
    let x = tree_label_x(row, entry.depth, -app.panels.tree.scroll_x);
    // 命中范围按完整宽度计算，绘制时才按当前宽度露出：过渡途中鼠标移过去不会落空。
    let right = view.map_or(rect.right(), |v| rect.right().min(x + v.width + 8.0));
    if right <= rect.x {
        return;
    }
    // 这一行在最上层：底色从滚动条的范围起，滚动条只被这一行的高度挡住，上下两段照常显示。
    let left = tree.right() - SCROLLBAR_HIT;
    let rect = Rect::new(left, rect.y, right - left, rect.h);
    let clip = Rect::new(rect.x, tree.y, rect.w, tree.h);
    let radii = [0.0, RADIUS_ROW, RADIUS_ROW, 0.0];
    let selected = ws.is_selected(index);
    let focused = app.focus == Focus::Tree;

    // 底层先垫一层侧栏底色，再叠半透明的选中色，使颜色与文件树中的那一段一致。
    frame.rect_corners(rect, p.sidebar_bg, radii, clip);

    // 收回时与侧栏内的那一段用同一个底色，一起淡出，两段不割裂。
    let tint = match view.filter(|_| closing) {
        Some(view) => retracting_color(p, selected, focused, view.progress),
        None => row_color(p, selected, focused, true),
    };
    if let Some(color) = tint {
        frame.rect_corners(rect, color, radii, clip);
    }

    let color = tree_entry_color(app, entry, selected);
    if let Some(view) = view.filter(|view| closing && !view.old.is_empty()) {
        // 收回中的内容：相同的前缀不淡，其余随过渡淡出。
        let step = chain_step(app, frame);
        let separator = color.alpha(color.a * CHAIN_SEPARATOR);
        let mut at = x;
        for (k, name) in view.old.iter().enumerate() {
            let alpha = if k < view.common { 1.0 } else { view.old_alpha };
            let width = frame.measure(FontKind::Ui, app.fonts.ui, name);
            ui_text(
                frame,
                app,
                at,
                row,
                name,
                color.alpha(color.a * alpha),
                clip,
            );
            if k + 1 < view.old.len() {
                let bar = separator.alpha(separator.a * alpha);
                ui_text(frame, app, at + width + CHAIN_GAP, row, "/", bar, clip);
            }
            at += width + step;
        }
        return;
    }
    if let Some(view) = view {
        draw_old_label(app, frame, (x, row), view, color, clip);
    }
    if entry.chain().is_empty() {
        ui_text(frame, app, x, row, &entry.label(), color, clip);
    } else {
        draw_chain(
            app,
            frame,
            entry,
            (x, row, None, view),
            (color, selected),
            clip,
            |_, _, _| {},
        );
    }
}

/// 返回名字收回途中这一行的底色：选中的行不变，其余是悬停的底色随进度 `progress` 淡出。
pub(in crate::view) fn retracting_color(
    p: &Palette,
    selected: bool,
    focused: bool,
    progress: f32,
) -> Option<Color> {
    row_color(p, selected, focused, false).or_else(|| {
        row_color(p, selected, focused, true).map(|color| color.alpha(color.a * (1.0 - progress)))
    })
}

/// 返回当前的一级在合并显示的目录中的下标：选中了其中某一级时是那一级，否则是最后一级；没有
/// 合并时为 0。
pub(super) fn chain_current(app: &App, entry: &Entry) -> usize {
    let chain = entry.chain();
    let segment = app.editor.workspace.as_ref().and_then(|w| w.segment());
    chain
        .iter()
        .position(|path| Some(path.as_path()) == segment)
        .unwrap_or(chain.len().saturating_sub(1))
}

/// 返回这一行过渡状态的键：合并显示时是最上一级，展开、收起前后不变。
pub(super) fn label_key(entry: &Entry) -> &Path {
    entry.chain().first().unwrap_or(&entry.path)
}

/// 返回折叠后一部分的显示名。
pub(super) fn part_name<'a>(entry: &'a Entry, part: Option<usize>) -> Cow<'a, str> {
    part.map_or(Cow::Borrowed(CHAIN_FOLD), |i| {
        winc_editor::segment_name(&entry.chain()[i])
    })
}

/// 返回相邻两部分的间距：分隔符和两侧的留白。
fn chain_step(app: &App, frame: &mut Frame<'_>) -> f32 {
    frame.measure(FontKind::Ui, app.fonts.ui, "/") + CHAIN_GAP * 2.0
}

/// 绘制过渡中淡出的旧内容，相同的前缀由新内容绘制，这里跳过。
pub(super) fn draw_old_label(
    app: &App,
    frame: &mut Frame<'_>,
    (start, row): (f32, Rect),
    view: &LabelView,
    color: Color,
    clip: Rect,
) {
    if view.old.is_empty() {
        return;
    }
    let step = chain_step(app, frame);
    let text = color.alpha(color.a * view.old_alpha);
    let separator = color.alpha(color.a * CHAIN_SEPARATOR * view.old_alpha);
    let mut x = start;
    for (k, name) in view.old.iter().enumerate() {
        let width = frame.measure(FontKind::Ui, app.fonts.ui, name);
        if k >= view.common {
            ui_text(frame, app, x, row, name, text, clip);
        }
        if k + 1 < view.old.len() && k + 1 > view.common {
            ui_text(frame, app, x + width + CHAIN_GAP, row, "/", separator, clip);
        }
        x += width + step;
    }
}

/// 返回这一行名称的宽度（逻辑像素）；合并显示的目录含各级之间的分隔符和留白。
pub(super) fn label_width(app: &App, frame: &mut Frame<'_>, entry: &Entry) -> f32 {
    let size = app.fonts.ui;
    let chain = entry.chain();
    if chain.is_empty() {
        return frame.measure(FontKind::Ui, size, &entry.name);
    }
    let names: f32 = chain
        .iter()
        .map(|path| frame.measure(FontKind::Ui, size, &winc_editor::segment_name(path)))
        .sum();
    let slash = frame.measure(FontKind::Ui, size, "/") + CHAIN_GAP * 2.0;
    names + slash * (chain.len() - 1) as f32
}

/// 逐级绘制合并显示的目录，起点是 `at` 的横坐标和所在的行；`report` 收到每一级的左右边界和路径。
///
/// 拖动放进其中某一级或鼠标悬停在某一级时，那一级加上底色并突出；选中这一行（`style` 的第二项）
/// 时突出选中的那一级。突出的一级用文字颜色（`style` 的第一项），其余按 [`CHAIN_DIM`] 变淡，被 Git
/// 忽略等本身较淡的文字也能分出是哪一级。分隔符和留白归入相邻的两级，各占一半。
///
/// `at` 的第三项是要显示的各部分（见 [`fold_chain`]），`None` 时显示全部；第四项是过渡的绘制
/// 参数，相同的前缀之后的部分按它淡入。
pub(super) fn draw_chain(
    app: &App,
    frame: &mut Frame<'_>,
    entry: &Entry,
    (start, row, parts, view): (f32, Rect, Option<&[Option<usize>]>, Option<&LabelView>),
    (color, selected): (Color, bool),
    clip: Rect,
    mut report: impl FnMut(f32, f32, &Path),
) {
    let ws = app.editor.workspace.as_ref();
    let chain = entry.chain();
    let dropping = app
        .tree_dragging()
        .and_then(|d| d.target.as_deref())
        .filter(|target| chain.iter().any(|path| path == target));
    let current = dropping
        .or_else(|| ws.and_then(|w| w.segment()))
        .unwrap_or(&entry.path);
    let emphasize = dropping.is_some() || selected && ws.is_some_and(|w| !w.multi_selected());
    let faded = color.alpha(color.a * CHAIN_DIM);
    let separator = color.alpha(color.a * CHAIN_SEPARATOR);
    let slash = frame.measure(FontKind::Ui, app.fonts.ui, "/");
    let step = slash + CHAIN_GAP * 2.0;
    let every: Vec<Option<usize>>;
    let parts = match parts {
        Some(parts) => parts,
        None => {
            every = (0..chain.len()).map(Some).collect();
            &every
        }
    };
    let mut x = start;
    for (k, part) in parts.iter().enumerate() {
        let fade = view
            .filter(|view| k >= view.common)
            .map_or(1.0, |view| view.alpha);
        let width = match part {
            Some(i) => {
                let path = &chain[*i];
                let name = winc_editor::segment_name(path);
                let width = frame.measure(FontKind::Ui, app.fonts.ui, &name);
                let hovered = dropping.is_none()
                    && matches!(app.pointer.hover, Hover::TreeRow(_))
                    && app.panels.tree.hover_segment.as_ref() == Some(path);
                if hovered || dropping == Some(path.as_path()) {
                    let area = Rect::new(
                        x - CHAIN_GAP,
                        row.y + 3.0,
                        width + CHAIN_GAP * 2.0,
                        row.h - 6.0,
                    );
                    frame.rect_clipped(area, app.palette.drop_target, RADIUS_ROW, clip);
                }
                let text = if emphasize && path != current && !hovered {
                    faded
                } else {
                    color
                };
                let text = if app.link.shared.cut_files.borrow().contains(path) {
                    text.alpha(text.a * 0.5)
                } else {
                    text
                };
                let text = text.alpha(text.a * fade);
                ui_text(frame, app, x, row, &name, text, clip);
                let left = if k == 0 { x } else { x - step / 2.0 };
                report(left, x + width + step / 2.0, path);
                width
            }
            None => {
                let mark = separator.alpha(separator.a * fade);
                ui_text(frame, app, x, row, CHAIN_FOLD, mark, clip);
                frame.measure(FontKind::Ui, app.fonts.ui, CHAIN_FOLD)
            }
        };
        if k + 1 < parts.len() {
            let bar = separator.alpha(separator.a * fade);
            ui_text(frame, app, x + width + CHAIN_GAP, row, "/", bar, clip);
        }
        x += width + step;
    }
}

/// 返回这一行折叠后要显示的各部分及其宽度（逻辑像素），放得进 `room` 时不折叠；不是合并显示的
/// 一行时返回 `None`。
pub(super) fn chain_folded(
    app: &App,
    frame: &mut Frame<'_>,
    entry: &Entry,
    room: f32,
) -> Option<(Vec<Option<usize>>, f32)> {
    let chain = entry.chain();
    if chain.is_empty() {
        return None;
    }
    let size = app.fonts.ui;
    let widths: Vec<f32> = chain
        .iter()
        .map(|path| frame.measure(FontKind::Ui, size, &winc_editor::segment_name(path)))
        .collect();
    let step = frame.measure(FontKind::Ui, size, "/") + CHAIN_GAP * 2.0;
    let fold = frame.measure(FontKind::Ui, size, CHAIN_FOLD);
    Some(fold_chain(
        &widths,
        step,
        fold,
        chain_current(app, entry),
        room,
    ))
}

/// 折叠放不进 `room` 的目录链，返回从左到右要显示的各部分（`Some` 是目录的下标，`None` 是一处折叠）
/// 和总宽度。
///
/// `widths` 是各级名字的宽度，`step` 是相邻两部分之间分隔符和留白的宽度，`fold` 是折叠记号的宽度。
/// 第一级和当前的一级 `current` 始终显示，最后一级放得下就显示；还有空间时从离 `current` 近的
/// 各级依次补上，距离相同时先补靠后的。
pub(in crate::view) fn fold_chain(
    widths: &[f32],
    step: f32,
    fold: f32,
    current: usize,
    room: f32,
) -> (Vec<Option<usize>>, f32) {
    let last = widths.len() - 1;
    let lay_out = |keep: &[bool]| {
        let mut parts = Vec::new();
        for (i, &kept) in keep.iter().enumerate() {
            if kept {
                parts.push(Some(i));
            } else if parts.last() != Some(&None) {
                parts.push(None);
            }
        }
        let width: f32 = parts
            .iter()
            .map(|part| part.map_or(fold, |i| widths[i]))
            .sum();
        let width = width + step * parts.len().saturating_sub(1) as f32;
        (parts, width)
    };

    let all = lay_out(&vec![true; widths.len()]);
    if all.1 <= room {
        return all;
    }
    let mut keep = vec![false; widths.len()];
    keep[0] = true;
    keep[current] = true;
    keep[last] = true;
    if lay_out(&keep).1 > room && current != last {
        keep[last] = false;
    }
    let mut order: Vec<usize> = (0..widths.len()).collect();
    order.sort_by_key(|&i| (i.abs_diff(current), std::cmp::Reverse(i)));
    for i in order {
        if keep[i] {
            continue;
        }
        keep[i] = true;
        if lay_out(&keep).1 > room {
            keep[i] = false;
            break;
        }
    }
    lay_out(&keep)
}
