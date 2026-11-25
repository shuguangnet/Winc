//! 编辑区顶置面包屑导航栏（VS Code Breadcrumbs）的绘制。
//!
//! 位于标签栏正下方、编辑区正文上方，依次显示相对路径各级目录、文件图标与文件名，
//! 以及配置文件当前键路径；点击任一目录段弹出同级文件快速跳转菜单。

use std::borrow::Cow;

use super::*;
use crate::icons::Direction;

/// 相邻面包屑段之间的间距（逻辑像素），中间绘制右箭头。
const CRUMB_GAP: f32 = 16.0;
/// 可点击面包屑段的左右悬停留白（逻辑像素）。
const CRUMB_PAD: f32 = 4.0;

/// 绘制编辑区顶置面包屑条并更新 `app.hits.crumbs`。
pub(in crate::view) fn draw_breadcrumbs(app: &mut App, frame: &mut Frame<'_>) {
    let bar = app.layout.breadcrumbs;
    if bar.w <= 0.0 || bar.h <= 0.0 {
        app.hits.crumbs.clear();
        return;
    }
    let p = app.palette;
    frame.rect(bar, p.editor_bg);

    let Some(doc) = app.editor.active() else {
        app.hits.crumbs.clear();
        return;
    };
    let crumbs = app.crumbs();
    let clip = Rect::new(bar.x + 8.0, bar.y, (bar.w - 16.0).max(0.0), bar.h);
    let mut x = clip.x + 4.0;
    let mut hits = Vec::new();
    let half = clip.w * 0.65;

    for (i, crumb) in crumbs.iter().enumerate() {
        let is_last = i + 1 == crumbs.len();
        let name = if crumbs.len() == 1 && crumb.path.is_some() {
            shorten_path(frame, &crumb.name, app.fonts.ui_small, half)
        } else {
            Cow::Borrowed(crumb.name.as_str())
        };
        if i > 0 {
            let arrow = (
                x + CRUMB_GAP / 2.0,
                icon_center_y(app, bar, app.fonts.ui_small),
            );
            icons::chevron(frame, arrow, 5.5, Direction::Right, p.text_dim, clip);
            x += CRUMB_GAP;
        }

        // 最后一段（文件名）左侧绘制小号文件类型图标，贴近 VS Code 面包屑样式。
        let icon_extra = if is_last {
            icons::FILE_WIDTH + 4.0
        } else {
            0.0
        };

        if crumb.path.is_some() {
            let w = frame.measure(FontKind::Ui, app.fonts.ui_small, &name) + icon_extra;
            let pill = Rect::new(x - CRUMB_PAD, bar.y + 2.0, w + 2.0 * CRUMB_PAD, bar.h - 4.0);
            let hovered = app.pointer.hover == Hover::Crumb(i);
            draw_button(
                frame,
                p,
                pill,
                clip,
                hovered,
                app.overlays.menu_button == Some(MenuButton::Crumb(i)),
            );
            if pill.x < clip.right() {
                let visible = pill.w.min(clip.right() - pill.x);
                hits.push((Rect::new(pill.x, pill.y, visible, pill.h), i));
            }
        }

        if is_last {
            let icon_name = tab_icon_name(doc);
            let cy = icon_center_y(app, bar, app.fonts.ui_small);
            draw_file_icon(frame, p, (x, cy), &icon_name, clip);
            x += icon_extra;
        }

        let color = if is_last { p.text } else { p.text_muted };
        x = ui_text_small(frame, app, x, bar, &name, color, clip);
    }

    // 配置文件光标所在键路径或大纲当前符号：接在文件名之后。
    if let Some(path) = app.key_path() {
        let arrow = (
            x + CRUMB_GAP / 2.0,
            icon_center_y(app, bar, app.fonts.ui_small),
        );
        icons::chevron(frame, arrow, 5.5, Direction::Right, p.text_dim, clip);
        x += CRUMB_GAP;
        let room = (clip.right() - x).max(0.0);
        let path = ellipsize(frame, &path, app.fonts.ui_small, room);
        ui_text_small(frame, app, x, bar, &path, p.text_muted, clip);
    }

    app.hits.crumbs = hits;
}
