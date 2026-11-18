//! 日志左侧的分支面板：「全部分支」，其余各行与分支选择器相同，正在看的那一个铺选中的底色。

use super::*;
use crate::app::git::log::{BRANCH_ROW, BRANCHES_W, BranchRow};
use crate::view::git::commit::draw_count_pill_in;

/// 绘制标题栏最左边显示、隐藏分支面板的按钮：面板显示着时按下的样子。
pub(super) fn draw_branches_button(
    app: &App,
    frame: &mut Frame<'_>,
    parts: &LogParts,
    hits: &mut Vec<(Rect, LogTarget)>,
) {
    let p = app.palette;
    let button = parts.branches_button();
    let hovered = app.pointer.hover == Hover::Log(LogTarget::BranchesToggle);
    // 按要切换到的状态画：关闭途中按钮已经弹起，打开途中已经按下。
    let on = app.state.log_branches.unwrap_or(true) && parts.branches.w > 0.0;
    draw_button(frame, p, button, parts.header, hovered, on);
    let ink = if hovered || on { p.text } else { p.text_muted };
    icons::sidebar(frame, center(button), on, ink, parts.header);
    hits.push((button, LogTarget::BranchesToggle));
}

/// 分支面板顶部搜索框的位置（逻辑像素）；面板没有完全展开时没有它：打开、关闭途中面板很窄，
/// 放不下搜索框，各行也按完整的宽度排、跟着右边缘滑进滑出。
pub(super) fn branch_field(parts: &LogParts) -> Option<Rect> {
    let area = parts.branches;
    if area.w < BRANCHES_W - 1.0 {
        return None;
    }
    // 与 `draw_branches` 中右对齐的 `slid` 相同。
    let x = area.right() - (BRANCHES_W - 1.0) + 8.0;
    Some(Rect::new(x, area.y + 6.0, BRANCHES_W - 1.0 - 16.0, 24.0))
}

/// 绘制分支面板顶部的搜索框（按分支名筛选面板里的各行）。
pub(super) fn draw_branch_field(
    app: &mut App,
    frame: &mut Frame<'_>,
    rect: Rect,
    hits: &mut Vec<(Rect, LogTarget)>,
) {
    let p = app.palette;
    let focused = app.focus == Focus::LogBranchFilter;
    field_box(app, frame, rect, p.editor_bg, focused, false);
    icons::magnifier(
        frame,
        (rect.x + 14.0, rect.y + rect.h / 2.0),
        p.text_dim,
        rect,
    );
    let area = Rect::new(rect.x + 26.0, rect.y, rect.w - 32.0, rect.h);
    let mut field = std::mem::take(&mut app.git.log.branch_filter);
    let caret = draw_bar_field(
        app,
        frame,
        area,
        &mut field,
        t!("git-log-branch-filter"),
        focused,
    );
    app.git.log.branch_filter = field;
    if focused {
        app.ime.cursor = caret;
    }
    hits.push((rect, LogTarget::BranchFilter));
}

/// 绘制分支面板，返回最大滚动距离（逻辑像素）；名字截短了的行记进 `long`。
///
/// 各行的排法和画法与分支选择器相同：分组标题是折叠箭头、小号淡色的组名和右端的数量徽标，分支
/// 名按 `/` 分出的目录有文件夹图标，当前分支在名字前打勾，其余分支、标签前面是淡色的记号。正在
/// 按它筛选的行铺选中的底色，没有按分支筛选时「全部分支」铺选中的底色；行的底色上下各让出 1，
/// 相邻两行之间留出间隔。组、目录展开、收起时子行逐渐露出（收回），下面的行随之移动。
pub(super) fn draw_branches(
    app: &App,
    frame: &mut Frame<'_>,
    view: &LogView,
    parts: &LogParts,
    (hits, long): (&mut Vec<(Rect, LogTarget)>, &mut Vec<usize>),
) -> f32 {
    let area = parts.branches;
    if area.w <= 0.0 {
        return 0.0;
    }
    let p = app.palette;
    frame.rect(
        Rect::new(area.right() - 1.0, area.y, 1.0, area.h),
        p.separator,
    );
    let area = Rect::new(area.x, area.y, area.w - 1.0, area.h);
    // 打开、关闭途中面板只露出右边一段：各行按完整的宽度排，跟着右边缘从左边滑进、滑出。
    let slid = Rect::new(
        area.right() - (BRANCHES_W - 1.0),
        area.y,
        BRANCHES_W - 1.0,
        area.h,
    );
    // 展开、收起途中按行数较多的那一版画（见 `PanelFold`）。
    let fold = app.git.log.branch_fold.as_ref();
    let rows = fold.map_or_else(|| app.log_branch_rows(), |f| f.rows.clone());
    let filter = &view.query.branches;
    // 搜索框下面才是各行：滚动时上一行不会盖住搜索框。
    let body = branch_field(parts).map_or(area, |field| {
        let y = field.bottom() + 4.0;
        Rect::new(area.x, y, area.w, (area.bottom() - y).max(0.0))
    });
    let top = body.y + LOG_LIST_PAD - app.git.log.branches_scroll;
    // 没有匹配的分支时给一句提示：否则面板看起来像空了一块（排出的行只剩分支选择器里的「新建
    // 分支」，面板不画它）。
    let blank = rows.iter().all(|row| matches!(row, BranchRow::Create(_)));
    if blank && !app.git.log.branch_filter.text().is_empty() {
        let band = Rect::new(slid.x + 6.0, top, slid.w - 12.0, BRANCH_ROW);
        ui_text_small(
            frame,
            app,
            band.x + 14.0,
            band,
            t!("menu-filter-no-match"),
            p.text_dim,
            body,
        );
    }
    for (i, row) in rows.iter().enumerate() {
        let (y, clip) = panel_slot(fold, (top, i), body);
        let band = Rect::new(slid.x + 6.0, y, slid.w - 12.0, BRANCH_ROW);
        if clip.h <= 0.0 || band.bottom() < clip.y || band.y > clip.bottom() {
            continue;
        }
        let target = LogTarget::BranchRow(i);
        // 动画期间行在移动，不画悬停、不记命中范围。
        let hovered = fold.is_none() && app.pointer.hover == Hover::Log(target);
        let named = |name: Option<&String>| name.is_some_and(|n| filter.contains(n));
        let chosen = match row {
            BranchRow::All => filter.is_empty(),
            BranchRow::Branch { index, .. } => named(app.git.branches.get(*index).map(|b| &b.name)),
            BranchRow::Tag(index) => named(app.git.tags.get(*index)),
            _ => false,
        };
        let shade = Rect::new(band.x, band.y + 1.0, band.w, band.h - 2.0);
        if let Some(color) = row_color(p, chosen, false, hovered) {
            frame.rect_clipped(shade, color, RADIUS_ROW, clip);
        }
        let cy = band.y + BRANCH_ROW / 2.0;
        // 图标一列和文字一列，每深一层缩进 `TREE_INDENT`（与分支选择器相同）。
        let columns = |depth: usize| {
            let indent = depth as f32 * crate::theme::TREE_INDENT;
            (band.x + 14.0 + indent, band.x + 26.0 + indent)
        };
        let right = band.right() - 8.0;
        let name = |frame: &mut Frame<'_>, x: f32, text: &str, long: &mut Vec<usize>| {
            // 名字和分支选择器一样用正文字号，组名、数量才用小号字。
            let shown = truncate_end(frame, text, app.fonts.ui, (right - x).max(0.0));
            if shown != text {
                long.push(i);
            }
            ui_text(frame, app, x, band, &shown, p.text, clip);
        };
        let turned = |open: bool| match fold {
            Some(f) if f.parent == i => f.reveal(),
            _ => f32::from(u8::from(open)),
        };
        let group = |frame: &mut Frame<'_>, title: &str, count: usize, open: bool| {
            // 和分支选择器一样，每组上方一条分隔线与上一组隔开。
            let line = Rect::new(band.x + 6.0, band.y, band.w - 12.0, 1.0);
            frame.rect_clipped(line, p.separator, 0.0, clip);
            let (arrow, text) = columns(0);
            let turn = turned(open) * FRAC_PI_2;
            icons::chevron_turned(frame, (arrow, cy), 6.0, turn, p.text_dim, clip);
            ui_text_small(frame, app, text, band, title, p.text_dim, clip);
            let pill = Rect::new(band.x, band.y, band.w, band.h);
            draw_count_pill_in(
                app,
                frame,
                pill,
                &count.to_string(),
                (p.hover, p.text_muted),
                clip,
            );
        };
        match row {
            BranchRow::All => {
                let text = columns(0).1;
                ui_text(
                    frame,
                    app,
                    text,
                    band,
                    t!("git-log-all-branches"),
                    p.text,
                    clip,
                );
            }
            BranchRow::Locals {
                recent,
                count,
                open,
            } => {
                let title = match recent {
                    true => t!("git-branch-picker-recent"),
                    false => t!("git-branch-picker-other"),
                };
                group(frame, title, *count, *open);
            }
            BranchRow::Remotes { count, open } => {
                group(frame, t!("git-branch-picker-remote"), *count, *open);
            }
            BranchRow::Tags { count, open } => {
                group(frame, t!("git-branch-picker-tags"), *count, *open);
            }
            // 分组里的目录：箭头在图标左边一层，文件夹图标和同层的分支对齐。
            BranchRow::Folder {
                label, depth, open, ..
            } => {
                let depth = depth + 1;
                let arrow = columns(depth - 1).0;
                let turn = turned(*open) * FRAC_PI_2;
                icons::chevron_turned(frame, (arrow, cy), 6.0, turn, p.text_dim, clip);
                let (icon, text) = columns(depth);
                icons::folder_small(frame, (icon, cy), p.text_dim, clip);
                name(frame, text, label, long);
            }
            // 当前分支打勾，其余分支前面是淡色的分支记号（远程分支用远程的记号）。
            BranchRow::Branch {
                index,
                depth,
                label,
            } => {
                let (icon, text) = columns(depth + 1);
                match app.git.branches.get(*index) {
                    Some(b) if b.current => icons::check(frame, (icon, cy), 9.0, p.text, clip),
                    Some(b) => {
                        let mark = match b.remote {
                            true => icons::RefMark::Remote,
                            false => icons::RefMark::Branch,
                        };
                        icons::ref_icon(frame, mark, (icon, cy), p.text_dim, clip);
                    }
                    None => {}
                }
                name(frame, text, label, long);
            }
            BranchRow::Tag(index) => {
                let (icon, text) = columns(1);
                icons::ref_icon(frame, icons::RefMark::Tag, (icon, cy), p.text_dim, clip);
                let tag = app.git.tags.get(*index).map_or("", String::as_str);
                name(frame, text, tag, long);
            }
            // 「新建分支」和「按输入的修订筛选」都只在选择器里，面板里不画。
            BranchRow::Create(_) | BranchRow::Revision(_) => {}
        }
        // 「新建分支」是分支选择器里的行，面板里不画它，也不记下点击范围：否则鼠标停在空行的
        // 位置会亮起一块没有内容的底色。
        if fold.is_none() && !matches!(row, BranchRow::Create(_) | BranchRow::Revision(_)) {
            hits.push((band.intersect(body), target));
        }
    }
    let content = LOG_LIST_PAD + rows.len() as f32 * BRANCH_ROW + LOG_LIST_PAD;
    (content - body.h).max(0.0)
}

/// 分支面板第 `i` 行的上沿和能画的范围：展开、收起途中子行位置不动、只露出已露出的一段，其后
/// 的行往上收还没露出的高度（与提交视图里组、目录的动画相同）。
fn panel_slot(
    fold: Option<&crate::app::git::log::PanelFold>,
    (top, i): (f32, usize),
    area: Rect,
) -> (f32, Rect) {
    let y = top + i as f32 * BRANCH_ROW;
    let Some(fold) = fold else {
        return (y, area);
    };
    let (first, last) = (fold.parent + 1, fold.parent + fold.count);
    if i > last {
        return (y - fold.hidden(), area);
    }
    if i < first {
        return (y, area);
    }
    let start = top + first as f32 * BRANCH_ROW;
    let shown = fold.count as f32 * BRANCH_ROW - fold.hidden();
    (
        y,
        Rect::new(area.x, start, area.w, shown.max(0.0)).intersect(area),
    )
}
