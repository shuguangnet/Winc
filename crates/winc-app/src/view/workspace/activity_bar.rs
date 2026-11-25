//! 最左侧 48px 垂直活动栏（VS Code Activity Bar）的绘制。
//!
//! 顶部依次排列资源管理器、全局搜索、源代码管理（带未提交改动数字角标）三个视图图标，
//! 当前激活视图左侧绘制 2px 强调色竖条；底部固定设置齿轮按钮。

use super::*;
use crate::layout::SidebarView;

/// 活动栏图标缩放比例（相对基础 14px 图标放大到约 19–20px，贴近 VS Code Codicon 观感）。
const ACTIVITY_ICON_SCALE: f32 = 1.32;

/// 绘制最左侧 48px 垂直活动栏及其右边缘 1px 分隔线。
pub(in crate::view) fn draw_activity_bar(app: &App, frame: &mut Frame<'_>) {
    let bar = app.layout.activity_bar;
    if bar.w <= 0.0 || bar.h <= 0.0 {
        return;
    }
    let p = app.palette;
    frame.rect(bar, p.activity_bar_bg);
    // 右边缘 1px 细分隔线，将活动栏与侧栏/编辑区分隔开。
    frame.rect(Rect::new(bar.right(), bar.y, 1.0, bar.h), p.border);

    let sidebar_open = app.sidebar_open();
    for (rect, view) in app.sidebar_tabs() {
        if rect.h <= 0.0 {
            continue;
        }
        let active = sidebar_open && app.panels.sidebar.view == view;
        let hovered = app.pointer.hover == Hover::ViewTab(view);
        let share = app.view_tab_share(view);
        let opacity = app.view_tab_opacity(view);

        // VS Code 标志性 2px 左侧激活指示条。
        if active && share >= 1.0 {
            let indicator = Rect::new(rect.x, rect.y, 2.0, rect.h);
            frame.rect_clipped(indicator, p.accent, 0.0, bar);
        }

        let base_color = if active || hovered {
            p.text
        } else {
            p.text_muted
        };
        let color = base_color.alpha(base_color.a * opacity);
        let scale = ACTIVITY_ICON_SCALE * (0.65 + 0.35 * share);
        let clip = rect.intersect(bar);
        let c = center(rect);
        match view {
            SidebarView::Files => icons::files(frame, c, scale, color, clip),
            SidebarView::Search => icons::search(frame, c, scale, color, clip),
            SidebarView::Commit => icons::git(frame, c, scale, color, clip),
        }

        // 源代码管理图标右下角绘制 VS Code 风格蓝色数字角标。
        if view == SidebarView::Commit
            && share >= 1.0
            && let Some(count) = app.commit_count_text()
        {
            draw_count_badge(app, frame, rect, &count, clip);
        }
    }

    // 底部设置齿轮图标（⌘,）。
    let gear = app.layout.activity_settings;
    if gear.h >= 24.0 {
        let active = app.settings.open;
        let hovered = app.pointer.hover == Hover::ActivitySettings;
        if active {
            frame.rect_clipped(Rect::new(gear.x, gear.y, 2.0, gear.h), p.accent, 0.0, bar);
        }
        let color = if active || hovered {
            p.text
        } else {
            p.text_muted
        };
        icons::gear_scaled(frame, center(gear), 1.18, color, gear.intersect(bar));
    }
}

/// 在源代码管理图标右下角绘制蓝色圆形/胶囊数字角标（如 `3`、`12`、`99+`）。
fn draw_count_badge(app: &App, frame: &mut Frame<'_>, cell: Rect, count: &str, clip: Rect) {
    let p = app.palette;
    let size = (app.fonts.ui_small - 2.0).max(9.0);
    let text_w = frame.measure(FontKind::Ui, size, count);
    let badge_h = 15.0;
    let badge_w = (text_w + 8.0).max(badge_h);
    let bx = (cell.x + cell.w / 2.0 + 3.0).min(cell.right() - badge_w - 4.0);
    let by = cell.y + cell.h / 2.0 + 2.0;
    let badge = Rect::new(bx.round(), by.round(), badge_w.round(), badge_h);
    frame.rect_clipped(badge, p.accent_fill, badge_h / 2.0, clip);
    caps_label(
        frame,
        app,
        size,
        (badge.x + (badge.w - text_w) / 2.0, badge),
        count,
        p.text_on_accent,
        clip,
    );
}
