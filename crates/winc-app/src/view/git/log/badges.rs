//! 提交的分支和标签徽章、作者头像。
//!
//! 徽章由提交的引用逐个生成（当前分支、本地分支、远程分支、标签），再按可用宽度排成一行；
//! 排不下的收成「+N」。头像按邮箱哈希取调色板里的颜色，同一个作者的颜色固定。

use super::*;
use crate::icons::RefMark;

/// 一个引用徽章。
#[derive(Debug, Clone, PartialEq)]
pub(in crate::view::git) struct Badge {
    /// 徽章上的文字；纯图标胶囊（例如已同步远程分支的紫色云朵圆徽章）为空串。
    pub text: String,
    /// 底色。
    pub fill: Color,
    /// 文字和图标的颜色。
    pub ink: Color,
    /// 文字前面的图标，表明引用的种类；收起的「+N」没有图标。
    pub mark: Option<RefMark>,
}

/// 纯图标圆形/胶囊徽章的固定宽度（逻辑像素）。
const ICON_ONLY_W: f32 = 17.0;

impl Badge {
    /// 返回徽章的宽度（逻辑像素）：纯图标徽章为固定圆宽，其余为文字宽 `text` 加两侧留白与图标位。
    fn width(&self, text: f32) -> f32 {
        if self.text.is_empty() && self.mark.is_some() {
            return ICON_ONLY_W;
        }
        text + self.pad()
    }

    /// 返回徽章中文字以外的宽度（逻辑像素）。
    pub(in crate::view::git) fn pad(&self) -> f32 {
        if self.text.is_empty() && self.mark.is_some() {
            return ICON_ONLY_W;
        }
        BADGE_PAD + self.mark.map_or(0.0, |_| BADGE_MARK)
    }
}

/// 徽章中文字两侧留白的总和（逻辑像素），两侧各一半。
const BADGE_PAD: f32 = 12.0;
/// 徽章中图标占的宽度（逻辑像素）：图标宽 9，与文字相隔 3。
const BADGE_MARK: f32 = 12.0;

/// 返回 VS Code 风格的 HEAD 蓝胶囊配色 `(fill, ink)`。
fn head_colors(p: &Palette) -> (Color, Color) {
    if p.dark {
        (Color::hex(0x3794FF), Color::hex(0x111111))
    } else {
        (p.accent_fill, p.text_on_accent)
    }
}

/// 返回 VS Code 风格的远程分支紫色云朵胶囊配色 `(fill, ink)`。
fn remote_colors(p: &Palette) -> (Color, Color) {
    if p.dark {
        (Color::hex(0xB180D7), Color::hex(0x181818))
    } else {
        (Color::hex(0x7C3AED), Color::hex(0xFFFFFF))
    }
}

/// 返回一个提交的各个引用徽章（1:1 对齐 VS Code Source Control Graph 风格）。
///
/// - 当前检出的 `HEAD` 分支：亮蓝胶囊 + 同心圆图标 `◎` + 分支名（如 `[◎ master]`）。
/// - 与本地分支同步在同一提交上的远程分支：紧邻其右绘一枚独立的紫色圆形云朵徽章 `[☁]`。
/// - 未与本地分支重合的远程分支：紫色胶囊 + 云朵图标 + 远程分支名。
/// - 其他本地分支：柔蓝胶囊 + 分支图标 + 分支名。
/// - 标签：暖琥珀胶囊 + 标签图标 + 标签名。
pub(in crate::view::git) fn badges(p: &Palette, commit: &Commit) -> Vec<Badge> {
    let local: Vec<&str> = commit
        .refs
        .iter()
        .filter_map(|r| match r {
            Ref::Head(n) | Ref::Branch(n) => Some(n.as_str()),
            _ => None,
        })
        .collect();
    let has_synced_remote = |name: &str| {
        commit.refs.iter().any(|r| match r {
            Ref::Remote(remote) => remote
                .split_once('/')
                .is_some_and(|(_, branch)| branch == name),
            _ => false,
        })
    };
    let soft = |c: Color| c.alpha(c.a * 0.18);
    let (head_fill, head_ink) = head_colors(p);
    let (remote_fill, remote_ink) = remote_colors(p);
    let mut out = Vec::new();
    for r in &commit.refs {
        match r {
            Ref::Detached => out.push(Badge {
                text: "HEAD".to_owned(),
                fill: head_fill,
                ink: head_ink,
                mark: Some(RefMark::Commit),
            }),
            Ref::Head(name) => {
                out.push(Badge {
                    text: name.clone(),
                    fill: head_fill,
                    ink: head_ink,
                    mark: Some(RefMark::Head),
                });
                if has_synced_remote(name) {
                    out.push(Badge {
                        text: String::new(),
                        fill: remote_fill,
                        ink: remote_ink,
                        mark: Some(RefMark::Remote),
                    });
                }
            }
            Ref::Branch(name) => {
                out.push(Badge {
                    text: name.clone(),
                    fill: soft(p.accent),
                    ink: p.accent,
                    mark: Some(RefMark::Branch),
                });
                if has_synced_remote(name) {
                    out.push(Badge {
                        text: String::new(),
                        fill: remote_fill,
                        ink: remote_ink,
                        mark: Some(RefMark::Remote),
                    });
                }
            }
            Ref::Remote(name) => {
                let branch = name.split_once('/').map_or(name.as_str(), |(_, b)| b);
                if local.contains(&branch) {
                    continue;
                }
                out.push(Badge {
                    text: name.clone(),
                    fill: soft(remote_fill),
                    ink: if p.dark {
                        remote_fill
                    } else {
                        Color::hex(0x6D28D9)
                    },
                    mark: Some(RefMark::Remote),
                });
            }
            Ref::Tag(name) => out.push(Badge {
                text: name.clone(),
                fill: soft(p.warning),
                ink: p.warning,
                mark: Some(RefMark::Tag),
            }),
        }
    }
    out
}

/// 把一行引用徽章排进宽度 `room`（逻辑像素），返回排好的徽章。
///
/// 每个徽章最宽 [`BADGE_MAX`]，放不下的从末尾截短；余下的连截短到 [`BADGE_MIN`] 都放不下
/// 时，收成一个淡色的「+N」。第一个徽章总要排上。
pub(in crate::view::git) fn fit_badges(
    app: &App,
    frame: &mut Frame<'_>,
    badges: &[Badge],
    room: f32,
) -> Vec<Badge> {
    let p = app.palette;
    let size = app.fonts.ui_small;
    let more_w = |frame: &mut Frame<'_>, n: usize| {
        frame.measure(FontKind::Ui, size, &format!("+{n}")) + BADGE_PAD
    };
    let mut out = Vec::new();
    let mut used = 0.0;
    for (i, badge) in badges.iter().enumerate() {
        let left = badges.len() - i - 1;
        // 后面还有徽章时给「+N」留出地方。
        let reserve = if left > 0 {
            more_w(frame, left) + BADGE_GAP
        } else {
            0.0
        };
        let fits = (room - used - reserve).min(BADGE_MAX);
        let full = badge.width(frame.measure(FontKind::Ui, size, &badge.text));
        // 纯图标云朵徽章只需极小宽度，只要剩余宽度容纳得下就直接保留。
        let min_need = if badge.text.is_empty() && badge.mark.is_some() {
            ICON_ONLY_W
        } else {
            BADGE_MIN
        };
        if i > 0 && full.min(BADGE_MAX) > fits && fits < min_need {
            let soft = p.text_muted.alpha(p.text_muted.a * 0.16);
            out.push(Badge {
                text: format!("+{}", badges.len() - i),
                fill: soft,
                ink: p.text_muted,
                mark: None,
            });
            return out;
        }
        let fits = if i == 0 {
            BADGE_MAX.min(room).max(min_need)
        } else {
            fits
        };
        let text = if badge.text.is_empty() {
            String::new()
        } else {
            let room = (fits - badge.pad()).max(0.0);
            truncate_end(frame, &badge.text, size, room).into_owned()
        };
        used += badge.width(frame.measure(FontKind::Ui, size, &text)) + BADGE_GAP;
        out.push(Badge {
            text,
            ..badge.clone()
        });
    }
    out
}

/// [`fit_badges`] 排好的一行徽章的总宽（逻辑像素），不含最后一个后面的间隔。
pub(in crate::view::git) fn badges_width(
    app: &App,
    frame: &mut Frame<'_>,
    badges: &[Badge],
) -> f32 {
    let size = app.fonts.ui_small;
    let total: f32 = badges
        .iter()
        .map(|b| b.width(frame.measure(FontKind::Ui, size, &b.text)) + BADGE_GAP)
        .sum();
    (total - BADGE_GAP).max(0.0)
}

/// 从 `x` 起画 [`fit_badges`] 排好的一行徽章。
pub(in crate::view::git) fn draw_badges(
    app: &App,
    frame: &mut Frame<'_>,
    (x, row): (f32, Rect),
    badges: &[Badge],
    clip: Rect,
) {
    let mut x = x;
    for badge in badges {
        x = draw_badge(app, frame, (x, row), badge, clip) + BADGE_GAP;
    }
}

/// 画一个 VS Code 风格的胶囊徽章，高为行高减 4（最多 17 逻辑像素）；纯图标徽章为圆形。返回右边缘的 x 坐标。
pub(super) fn draw_badge(
    app: &App,
    frame: &mut Frame<'_>,
    (x, row): (f32, Rect),
    badge: &Badge,
    clip: Rect,
) -> f32 {
    let icon_only = badge.text.is_empty() && badge.mark.is_some();
    let h = (row.h - 4.0).clamp(15.0, 17.0);
    let w = if icon_only {
        h
    } else {
        badge.width(frame.measure(FontKind::Ui, app.fonts.ui_small, &badge.text))
    };
    let rect = Rect::new(x, (row.y + (row.h - h) / 2.0).round(), w, h);
    let radius = h / 2.0;
    frame.rect_clipped(rect, badge.fill, radius, clip);
    // 半透明底色的引用徽章（非 HEAD 本地分支、远程分支、标签）加一圈细描边，在选中行底色上依然边界清晰。
    if badge.fill.a < 0.9 {
        let outline = icons::rounded_outline(
            rect.x + 0.5,
            rect.y + 0.5,
            rect.w - 1.0,
            rect.h - 1.0,
            (radius - 0.5).max(1.0),
        );
        frame.stroke(&outline, 1.0, badge.ink.alpha(0.36), clip);
    }
    if icon_only {
        if let Some(mark) = badge.mark {
            let center = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            icons::ref_mark(frame, mark, center, badge.ink, clip);
        }
        return rect.right();
    }
    let mut text_x = x + BADGE_PAD / 2.0;
    if let Some(mark) = badge.mark {
        let center = (text_x + 4.5, rect.y + rect.h / 2.0);
        icons::ref_mark(frame, mark, center, badge.ink, clip);
        text_x += BADGE_MARK;
    }
    ui_text_small(frame, app, text_x, rect, &badge.text, badge.ink, clip);
    rect.right()
}

/// 绘制详情栏左上角的作者大头像（对齐 `gitHistoryVSCode` `.authorAndCommitInfoContainer img.avatar`）。
pub(super) fn draw_large_avatar(
    app: &App,
    frame: &mut Frame<'_>,
    commit: &Commit,
    rect: Rect,
    clip: Rect,
) {
    let p = app.palette;
    let hash = commit
        .email
        .bytes()
        .fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b));
    let (fill, ink) = p.badges[hash as usize % p.badges.len()];
    frame.rect_clipped(rect, fill, 6.0, clip);
    let initial: String = commit
        .author
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    let size = (rect.h * 0.44).clamp(18.0, 28.0);
    let w = frame.measure(FontKind::Ui, size, &initial);
    let cx = rect.x + rect.w / 2.0;
    caps_label(frame, app, size, (cx - w / 2.0, rect), &initial, ink, clip);
}

/// 在提交行右侧绘制 `gitHistoryVSCode` 风格的引用标签列表（顺序：远程分支 → 本地分支/HEAD → 标签）。
///
/// 1:1 对齐 `Refs/Remote.tsx`、`Refs/Head.tsx`、`Refs/Tag.tsx` 与 `main.css`：
/// - 远程分支（`.commit-remote-container`）：红色圆角块 + 分支图标 + 名称 + `| ×`
/// - 本地分支 / HEAD（`.commit-head-container`）：绿色圆角块 + 分支图标 + 名称（可点击签出）+ `| ×`
/// - 标签（`.commit-tag-container`）：灰色圆角块 + 标签图标 + 名称 + `| ×`
///
/// 返回整组引用标签的最左 x 坐标（没有引用时返回 `right_x`）。
pub(super) fn draw_githistory_refs(
    app: &App,
    frame: &mut Frame<'_>,
    commit: &Commit,
    row_idx: usize,
    (right_x, y, max_w): (f32, f32, f32),
    clip: Rect,
    hits: &mut Vec<(Rect, LogTarget)>,
) -> f32 {
    if commit.refs.is_empty() || max_w < 30.0 {
        return right_x;
    }
    let p = app.palette;
    let size = app.fonts.ui_small * 0.92;
    let pill_h = 19.0;
    let gap = 4.0;
    let white = Color::hex(0xFFFFFF);
    let remove_red = Color::hex(0xFF7A7A);

    // 1:1 对齐 `LogEntry/index.tsx:122-124` 的显示顺序：Remote -> Head/Branch -> Tag。
    let mut ordered: Vec<(usize, &Ref)> = Vec::with_capacity(commit.refs.len());
    for (idx, r) in commit.refs.iter().enumerate() {
        if matches!(r, Ref::Remote(_)) {
            ordered.push((idx, r));
        }
    }
    for (idx, r) in commit.refs.iter().enumerate() {
        if matches!(r, Ref::Head(_) | Ref::Branch(_) | Ref::Detached) {
            ordered.push((idx, r));
        }
    }
    for (idx, r) in commit.refs.iter().enumerate() {
        if matches!(r, Ref::Tag(_)) {
            ordered.push((idx, r));
        }
    }

    // 计算每个徽章的配色、文字、图标和是否带 `×` / 可签出。
    struct PillSpec<'a> {
        ref_idx: usize,
        label: &'a str,
        fill: Color,
        mark: RefMark,
        checkout: bool,
        removable: bool,
    }
    let green = if p.dark {
        Color::hex(0x387A2A)
    } else {
        Color::hex(0x2E7D32)
    };
    let red = if p.dark {
        Color::hex(0xB3362F)
    } else {
        Color::hex(0xC62828)
    };
    let gray = if p.dark {
        Color::hex(0x5A5A5A)
    } else {
        Color::hex(0x616161)
    };

    let specs: Vec<PillSpec<'_>> = ordered
        .into_iter()
        .map(|(ref_idx, r)| match r {
            Ref::Remote(name) => PillSpec {
                ref_idx,
                label: name.as_str(),
                fill: red,
                mark: RefMark::Branch,
                checkout: false,
                removable: true,
            },
            Ref::Head(name) => PillSpec {
                ref_idx,
                label: name.as_str(),
                fill: green,
                mark: RefMark::Branch,
                checkout: false,
                removable: false,
            },
            Ref::Detached => PillSpec {
                ref_idx,
                label: "HEAD",
                fill: green,
                mark: RefMark::Commit,
                checkout: false,
                removable: false,
            },
            Ref::Branch(name) => PillSpec {
                ref_idx,
                label: name.as_str(),
                fill: green,
                mark: RefMark::Branch,
                checkout: true,
                removable: true,
            },
            Ref::Tag(name) => PillSpec {
                ref_idx,
                label: name.as_str(),
                fill: gray,
                mark: RefMark::Tag,
                checkout: false,
                removable: true,
            },
        })
        .collect();

    // 从右往左依次绘制，确保靠右对齐，且总宽不超过 `max_w`。
    let mut cursor_right = right_x;
    let min_left = right_x - max_w;
    for spec in specs.iter().rev() {
        let remove_w = if spec.removable { 18.0 } else { 0.0 };
        let text_max = (140.0_f32).min(cursor_right - min_left - 24.0 - remove_w);
        if text_max < 24.0 {
            break;
        }
        let shown = truncate_end(frame, spec.label, size, text_max);
        let text_w = frame.measure(FontKind::Ui, size, &shown);
        let pill_w = 6.0 + 11.0 + text_w + 6.0 + remove_w;
        let x = cursor_right - pill_w;
        if x < min_left {
            break;
        }
        let rect = Rect::new(x, y, pill_w, pill_h);
        frame.rect_clipped(rect, spec.fill, 5.0, clip);
        let cy = rect.y + rect.h / 2.0;
        icons::ref_mark(frame, spec.mark, (x + 9.0, cy), white, clip);
        let (ascent, descent) = (app.metrics.ui.ascent, app.metrics.ui.descent);
        let scale = size / app.fonts.ui;
        let baseline = baseline_in(rect, ascent * scale, descent * scale);
        frame.text(FontKind::Ui, size, x + 16.0, baseline, &shown, white, clip);
        if spec.checkout {
            let body = Rect::new(x, y, pill_w - remove_w, pill_h).intersect(clip);
            hits.push((body, LogTarget::RefCheckout(row_idx, spec.ref_idx)));
        }
        if spec.removable {
            let sep_x = rect.right() - remove_w;
            frame.rect_clipped(
                Rect::new(sep_x, y + 3.0, 1.0, pill_h - 6.0),
                white.alpha(0.32),
                0.0,
                clip,
            );
            let close_rect = Rect::new(sep_x + 1.0, y, remove_w - 1.0, pill_h);
            let hovered =
                app.pointer.hover == Hover::Log(LogTarget::RefRemove(row_idx, spec.ref_idx));
            let close_ink = if hovered { white } else { remove_red };
            icons::close(frame, center(close_rect), 6.0, close_ink, clip);
            hits.push((
                close_rect.intersect(clip),
                LogTarget::RefRemove(row_idx, spec.ref_idx),
            ));
        }
        cursor_right = x - gap;
    }
    cursor_right
}

/// 作者的小头像：一个按邮箱取色的圆，里面是名字的第一个字。
///
/// 邮箱按 djb2 哈希（初值 5381、乘 33 再异或）取调色板里的颜色，同一个作者的颜色固定。
pub(super) fn draw_avatar(
    app: &App,
    frame: &mut Frame<'_>,
    commit: &Commit,
    (cx, cy): (f32, f32),
    clip: Rect,
) {
    let p = app.palette;
    let hash = commit
        .email
        .bytes()
        .fold(5381u32, |h, b| h.wrapping_mul(33) ^ u32::from(b));
    let (fill, ink) = p.badges[hash as usize % p.badges.len()];
    // 头像的半径（逻辑像素，对齐 `gitHistoryVSCode` `15x15` `.avatar`）。
    let r = 7.5;
    frame.rect_clipped(Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r), fill, r, clip);
    let initial: String = commit
        .author
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    let size = app.fonts.ui_small * 0.8;
    let w = frame.measure(FontKind::Ui, size, &initial);
    let rect = Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r);
    caps_label(frame, app, size, (cx - w / 2.0, rect), &initial, ink, clip);
}
