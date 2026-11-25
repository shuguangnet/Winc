//! 顶部全宽标题栏（VS Code Title Bar）的绘制：居中命令中心搜索条与右侧布局切换按钮。

use super::*;

/// 绘制顶部 35px 全宽标题栏（底色 `title_bar_bg`、底边 1px `border`）、居中命令中心搜索条，
/// 以及右侧「切换主侧栏 / 切换终端面板 / 向右拆分编辑器」三个布局控制按钮。
pub(in crate::view) fn draw_title_bar(app: &App, frame: &mut Frame<'_>) {
    let bar = app.layout.title_bar;
    if bar.w <= 0.0 || bar.h <= 0.0 {
        return;
    }
    let p = app.palette;
    frame.rect(bar, p.title_bar_bg);
    frame.rect(Rect::new(bar.x, bar.bottom() - 1.0, bar.w, 1.0), p.border);

    draw_command_center(app, frame, bar);
    draw_layout_controls(app, frame, bar);
}

/// 绘制居中的 VS Code 命令中心搜索条（点击唤起快速打开 / 命令面板）。
fn draw_command_center(app: &App, frame: &mut Frame<'_>, bar: Rect) {
    let cc = app.layout.command_center;
    if cc.w <= 0.0 || cc.h <= 0.0 {
        return;
    }
    let p = app.palette;
    let hovered = app.pointer.hover == Hover::CommandCenter;
    let border = if hovered {
        p.accent.alpha(0.6)
    } else {
        p.border
    };
    let fill = if hovered {
        panel::over(p.editor_bg, p.hover)
    } else {
        p.editor_bg
    };
    frame.rect_clipped(cc, border, 6.0, bar);
    frame.rect_clipped(cc.inset(1.0, 1.0), fill, 5.0, bar);

    let title = app
        .editor
        .root()
        .and_then(|r| r.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Winc".to_owned());
    let size = app.fonts.ui_small;
    let max_text = (cc.w - 34.0).max(0.0);
    let shown = ellipsize(frame, &title, size, max_text);
    let text_w = frame.measure(FontKind::Ui, size, &shown);
    let icon_w = 12.0;
    let gap = 6.0;
    let total = icon_w + gap + text_w;
    let start_x = (cc.x + (cc.w - total) / 2.0).round();
    let cy = cc.y + cc.h / 2.0;
    let ink = if hovered { p.text } else { p.text_muted };
    icons::search(frame, (start_x + icon_w / 2.0, cy), 0.78, ink, cc);
    ui_text_small(frame, app, start_x + icon_w + gap, cc, &shown, ink, cc);
}

/// 绘制标题栏右侧的三个 VS Code 布局切换按钮：主侧栏、底部面板/终端、向右拆分编辑器。
fn draw_layout_controls(app: &App, frame: &mut Frame<'_>, bar: Rect) {
    let p = app.palette;

    // 1. 切换主侧栏
    let sb = app.layout.sidebar_button;
    if sb.w > 0.0 {
        let hovered = app.pointer.hover == Hover::SidebarButton;
        let open = app.sidebar_open();
        draw_button(frame, p, sb, bar, hovered, false);
        let color = if hovered || open {
            p.text
        } else {
            p.text_muted
        };
        icons::sidebar(frame, center(sb), open, color, sb);
    }

    // 2. 切换终端/底部面板
    let pb = app.layout.panel_button;
    if pb.w > 0.0 {
        let hovered = app.pointer.hover == Hover::PanelButton;
        let open = app.layout.terminal.h > 0.0 && app.layout.terminal.w > 0.0;
        draw_button(frame, p, pb, bar, hovered, open);
        let color = if hovered || open {
            p.text
        } else {
            p.text_muted
        };
        icons::panel(frame, center(pb), false, color, pb);
    }

    // 3. 向右拆分编辑器
    let sp = app.layout.split_button;
    if sp.w > 0.0 {
        let enabled = app.editor.active.is_some();
        let hovered = enabled && app.pointer.hover == Hover::SplitButton;
        draw_button(frame, p, sp, bar, hovered, false);
        let color = if !enabled {
            p.text_dim
        } else if hovered {
            p.text
        } else {
            p.text_muted
        };
        icons::split(frame, center(sp), color, sp);
    }
}
