//! 设置和开始页共用的控件的绘制：点击后弹出菜单的值、开关、分段控件的底槽和凸起的一段。
//!
//! 这里只绘制，控件的位置、状态和点击区域由调用方决定。

use super::*;

/// 绘制点击后弹出菜单的值：平时是一段右对齐的淡色文本（未选择时 `dim` 为真，颜色更淡），
/// 其后一对上下小箭头表示可选；悬停时铺悬停底色。
///
/// 不绘制边框，样式与字体名等其他设置项一致。
pub(in crate::view) fn popup_value(
    app: &App,
    frame: &mut Frame<'_>,
    rect: Rect,
    text: &str,
    dim: bool,
    hovered: bool,
    clip: Rect,
) {
    let p = app.palette;
    if hovered {
        frame.rect_clipped(rect, p.hover, RADIUS_ROW, clip);
    }
    let area = Rect::new(
        rect.x + 10.0,
        rect.y,
        (rect.w - 10.0 - SETTINGS_GLYPH_PAD).max(0.0),
        rect.h,
    );
    let arrows = (area.right() - 3.0, rect.y + rect.h / 2.0);
    icons::up_down(frame, arrows, p.text_dim, clip);

    let room = Rect::new(area.x, area.y, (area.w - 16.0).max(0.0), area.h);
    let text = ellipsize(frame, text, app.fonts.ui, room.w);
    let w = frame.measure(FontKind::Ui, app.fonts.ui, &text);
    let color = if dim { p.text_dim } else { p.text_muted };
    ui_text(frame, app, room.right() - w, room, &text, color, room);
}

/// 绘制开关 `track`：圆角底色，开启时为强调色、圆钮在右侧，关闭时为灰色、圆钮在左侧。
///
/// `t` 是开启程度：0 为关闭，1 为开启，切换途中在两者之间，圆钮和强调色随之过渡。
pub(in crate::view) fn switch(frame: &mut Frame<'_>, p: &Palette, track: Rect, t: f32, clip: Rect) {
    let radius = track.h / 2.0;
    frame.rect_clipped(track, p.switch_off, radius, clip);
    if t > 0.0 {
        frame.rect_clipped(track, p.accent.alpha(p.accent.a * t), radius, clip);
    }
    let knob = track.h - 4.0;
    let left = track.x + 2.0;
    let x = left + (track.right() - 2.0 - knob - left) * t;
    let knob = Rect::new(x, track.y + 2.0, knob, knob);
    frame.rect_clipped(knob, p.switch_knob, knob.h / 2.0, clip);
}

/// 绘制分段控件的底槽 `track` 和其中凸起的一段 `slot`（没有选中项时为 `None`），各段的文字由
/// 调用方压在上面。`under` 是控件下面的底色，凸起段的颜色据此算出。
pub(in crate::view) fn segment_track(
    frame: &mut Frame<'_>,
    p: &Palette,
    track: Rect,
    under: Color,
    slot: Option<Rect>,
    clip: Rect,
) {
    frame.rect_clipped(track, p.hover, RADIUS_ROW + 2.0, clip);
    let track_bg = super::workspace::panel::over(under, p.hover);
    let (raised, edge) = super::workspace::panel::segment_colors(p, track_bg);
    if let Some(slot) = slot {
        frame.rect_clipped(slot, edge, RADIUS_ROW, clip);
        frame.rect_clipped(slot.inset(0.5, 0.5), raised, RADIUS_ROW - 0.5, clip);
    }
}

/// 绘制一条 1 逻辑像素宽的垂直缩进参考线；`active` 为真时使用强调后的高亮颜色，用于标识当前
/// 焦点/悬停/选中项所属的层级。
pub(in crate::view) fn indent_guide(
    frame: &mut Frame<'_>,
    p: &Palette,
    x: f32,
    y: f32,
    h: f32,
    active: bool,
    clip: Rect,
) {
    let color = if active {
        p.indent_guide_active
    } else {
        p.indent_guide
    };
    frame.rect_clipped(Rect::new(x.round(), y, 1.0, h), color, 0.0, clip);
}

/// 绘制柔和圆角胶囊徽章：填充 `fill` 底色后在水平留白 `pad_x` 处以小号 UI 字体绘制 `text`。
pub(in crate::view) fn soft_pill(
    app: &App,
    frame: &mut Frame<'_>,
    rect: Rect,
    text: &str,
    (color, fill, radius): (Color, Color, f32),
    pad_x: f32,
    clip: Rect,
) {
    frame.rect_clipped(rect, fill, radius, clip);
    ui_text_small(frame, app, rect.x + pad_x, rect, text, color, clip);
}

/// 绘制右侧计数胶囊徽章：半圆角底色配居中数字文本。
pub(in crate::view) fn count_pill(
    app: &App,
    frame: &mut Frame<'_>,
    rect: Rect,
    text: &str,
    pad_x: f32,
    clip: Rect,
) {
    let p = app.palette;
    frame.rect_clipped(rect, p.hover, rect.h / 2.0, clip);
    caps_label(
        frame,
        app,
        app.fonts.ui_small,
        (rect.x + pad_x, rect),
        text,
        p.text_muted,
        clip,
    );
}
