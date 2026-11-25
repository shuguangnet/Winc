//! 详情栏（1:1 还原 `gitHistoryVSCode` `components/LogView/Commit/index.tsx`）：
//!
//! 左侧大号作者头像（64×64），中间为大字号提交标题（带 `⧉` 复制完整提交信息）、
//! `[Eye]` 作者/邮箱/时间、短提交号与父提交链接、所在分支以及完整提交正文；
//! 右上角固定放置 `Find file` 输入框、树形/列表切换按钮与 `×` 关闭详情按钮；
//! 下方为 `.comitted-files` 改动文件列表（5 格增删色块 + 状态 + 相对路径 + 右侧 4 项显式操作链接）。

use std::ops::Range;
use std::rc::Rc;

use winc_i18n::t;

use super::*;
use crate::app::git::log::Wrapped;

/// 提示中「单击比较」所按修饰键的写法。
const COMMAND: &str = if cfg!(target_os = "macos") {
    "⌘"
} else {
    "Ctrl"
};

/// 详情右上角工具区预留的总宽度（逻辑像素）：`Find file` 输入框 + 视图切换 + 关闭按钮。
fn top_actions_width(app: &App, details_w: f32) -> f32 {
    let flat = app.git.commit.flat();
    let btns = if flat { 2.0 } else { 3.0 };
    let input_w = (details_w * 0.24).clamp(110.0, 175.0);
    input_w + btns * 28.0 + 12.0
}

/// 详情右上角「Find file」输入框的位置（窗口坐标，逻辑像素）。
pub(super) fn inline_file_filter_rect(app: &App, parts: &LogParts) -> Rect {
    let area = parts.details;
    let flat = app.git.commit.flat();
    let btns_w = if flat { 2.0 * 28.0 } else { 3.0 * 28.0 };
    let input_w = (area.w * 0.24).clamp(110.0, 175.0);
    let x = (area.right() - PAD - btns_w - 6.0 - input_w).max(area.x + PAD);
    Rect::new(x, area.y + 10.0, input_w, 24.0)
}

/// 绘制详情右上角的固定操作按钮（树形/平铺切换、折叠全部目录、`×` 关闭详情视图）。
///
/// 1:1 对齐 `Commit/index.tsx:116-136` 的 `div.actions`。
pub(super) fn draw_details_top_actions(
    app: &App,
    frame: &mut Frame<'_>,
    view: &LogView,
    parts: &LogParts,
    hits: &mut Vec<(Rect, LogTarget)>,
) {
    let p = app.palette;
    let area = parts.details;
    let y = area.y + 10.0;
    let size = 24.0;
    // 用背景色垫底，防止详情滚动时下方文字透出到固定操作区。
    let filter_rect = inline_file_filter_rect(app, parts);
    let backdrop = Rect::new(
        filter_rect.x - 6.0,
        area.y + 2.0,
        area.right() - filter_rect.x + 6.0,
        36.0,
    )
    .intersect(area);
    frame.rect(backdrop, p.editor_bg);

    // 最右：`[×]` 关闭详情视图（比较模式下等同于退出比较）。
    let close_rect = Rect::new(area.right() - PAD - size, y, size, size);
    let close_target = if view.compare.is_some() {
        LogTarget::CompareExit
    } else {
        LogTarget::CloseDetails
    };
    let hovered = app.pointer.hover == Hover::Log(close_target);
    draw_button(frame, p, close_rect, area, hovered, false);
    let ink = if hovered { p.text } else { p.text_muted };
    icons::close(frame, center(close_rect), 8.0, ink, area);
    hits.push((close_rect.intersect(area), close_target));

    // 左侧：`Tree / List` 文件视图切换。
    let flat = app.git.commit.flat();
    let layout_rect = Rect::new(close_rect.x - 4.0 - size, y, size, size);
    let hovered = app.pointer.hover == Hover::Log(LogTarget::FilesLayout);
    draw_button(frame, p, layout_rect, area, hovered, false);
    let ink = if hovered { p.text } else { p.text_muted };
    match flat {
        true => icons::tree_view(frame, center(layout_rect), ink, area),
        false => icons::list_view(frame, center(layout_rect), ink, area),
    }
    hits.push((layout_rect.intersect(area), LogTarget::FilesLayout));

    // 树形模式下提供「折叠/展开全部目录」按钮。
    if !flat {
        let fold_rect = Rect::new(layout_rect.x - 4.0 - size, y, size, size);
        let hovered = app.pointer.hover == Hover::Log(LogTarget::FilesFoldAll);
        draw_button(frame, p, fold_rect, area, hovered, false);
        let ink = if hovered { p.text } else { p.text_muted };
        icons::collapse_all(frame, center(fold_rect), ink, area);
        hits.push((fold_rect.intersect(area), LogTarget::FilesFoldAll));
    }
}

/// 说明 `message` 按宽度 `width`（逻辑像素）折好的行；与上次的结果相同时直接复用缓存。
fn wrapped(app: &App, frame: &mut Frame<'_>, message: &str, width: f32) -> Rc<Wrapped> {
    let (ui, small) = (app.fonts.ui, app.fonts.ui_small);
    let key = (width.to_bits(), ui.to_bits(), small.to_bits());
    let mut cache = app.git.log.wrapped.borrow_mut();
    if let Some(w) = cache.as_ref()
        && w.key.0 == message
        && (w.key.1, w.key.2, w.key.3) == key
    {
        return Rc::clone(w);
    }
    let (subject, body) = message.split_once('\n').unwrap_or((message, ""));
    let (subject, body) = (subject.trim(), body.trim());
    let ranges = |base: &str, lines: Vec<&str>| -> Vec<Range<usize>> {
        let at = |s: &str| s.as_ptr() as usize - base.as_ptr() as usize;
        lines.into_iter().map(|l| at(l)..at(l) + l.len()).collect()
    };
    let wrapped = Rc::new(Wrapped {
        key: (message.to_owned(), key.0, key.1, key.2),
        subject: ranges(subject, wrap(frame, ui, subject, width)),
        body: if body.is_empty() {
            Vec::new()
        } else {
            (body.lines())
                .map(|paragraph| ranges(paragraph, wrap(frame, small, paragraph, width)))
                .collect()
        },
    });
    *cache = Some(Rc::clone(&wrapped));
    wrapped
}

/// 绘制详情一栏（1:1 对齐 `gitHistoryVSCode` `#detail-view`）。
///
/// 返回内容底部的位置（逻辑像素）。
pub(super) fn draw_details(
    app: &App,
    frame: &mut Frame<'_>,
    view: &LogView,
    parts: &LogParts,
    (hits, long_files, runs): (
        &mut Vec<(Rect, LogTarget)>,
        &mut Vec<usize>,
        &mut Vec<TextRun>,
    ),
) -> f32 {
    let p = app.palette;
    let area = parts.details;
    let top = area.y - view.details_scroll;
    if view.commits.is_empty() {
        return top;
    }
    let footer_h = if area.h >= 60.0 { 30.0 } else { 0.0 };
    let content_area = Rect::new(area.x, area.y, area.w, (area.h - footer_h).max(0.0));
    if let Some(compare) = &view.compare {
        return draw_compare(app, frame, view, compare, content_area, (hits, long_files));
    }
    let Some(selected) = view.selected.and_then(|i| view.commits.get(i)) else {
        draw_empty_state(
            app,
            frame,
            (content_area, content_area),
            t!("git-log-no-selection"),
            Some(t!("git-log-no-selection-detail")),
        );
        return top;
    };

    let current = view.details.as_ref().filter(|d| d.oid == selected.oid);
    let held = held_details(app, view, selected);
    let mut ignored = Vec::new();
    let (commit, details, hits) = match held {
        Some((commit, d)) => (commit, Some(d), &mut ignored),
        None => (selected, current, hits),
    };
    let x = content_area.x + PAD;
    let width = (content_area.w - 2.0 * PAD).max(0.0);
    let mut y = content_area.y + details_top(app.fonts.line_height) - view.details_scroll;

    // 1. `.authorAndCommitInfoContainer`：左侧 64×64 大头像 + 中间标题/作者/正文 + 右上预留给 `Find file` 和 `×`。
    let show_big_avatar = width >= 460.0;
    let avatar_s = 60.0;
    let info_x = if show_big_avatar {
        x + avatar_s + 12.0
    } else {
        x
    };
    let actions_reserve = top_actions_width(app, content_area.w);
    let info_w = (x + width - info_x - actions_reserve).max(120.0);
    if show_big_avatar {
        let av_rect = Rect::new(x, y, avatar_s, avatar_s);
        draw_large_avatar(app, frame, commit, av_rect, content_area);
    }

    // 说明：第一行用正文字号，其余淡色小字，按宽度折行，支持鼠标拖选复制。
    let raw_message = details
        .and_then(|d| d.message.as_deref())
        .unwrap_or(&commit.subject);
    let emojified = gitmojify(raw_message);
    let message = emojified.as_ref();
    let (subject, body) = message.split_once('\n').unwrap_or((message, ""));
    let (subject, body) = (subject.trim(), body.trim());
    let wrapped = wrapped(app, frame, message, (info_w - 26.0).max(80.0));
    let info_top = y;
    let mut last_subject_end = info_x;
    let mut last_subject_row = Rect::new(info_x, y, info_w, 24.0);
    for (i, line) in wrapped.subject.iter().enumerate() {
        let row = Rect::new(info_x, y, info_w, 24.0);
        let style = (app.fonts.ui, p.text, i > 0, None);
        last_subject_end = selectable(
            app,
            frame,
            runs,
            (info_x, row),
            &subject[line.clone()],
            style,
            content_area,
        );
        last_subject_row = row;
        y += 24.0;
    }
    // 标题末尾紧邻 `[⧉ Copy commit text]` 图标按钮（对齐 `Commit/index.tsx:102-110`）。
    let copy_msg_btn = Rect::new(
        (last_subject_end + 6.0).min(info_x + info_w - 20.0),
        last_subject_row.y + 3.0,
        18.0,
        18.0,
    );
    let hovered = app.pointer.hover == Hover::Log(LogTarget::CopyCommitText);
    draw_button(frame, p, copy_msg_btn, content_area, hovered, false);
    icons::copy(
        frame,
        center(copy_msg_btn),
        if hovered { p.text } else { p.text_muted },
        content_area,
    );
    hits.push((
        copy_msg_btn.intersect(content_area),
        LogTarget::CopyCommitText,
    ));

    // 作者行：`[Eye]` + 作者名 + `<email>` + `on <local>`（相对时间）。
    let author_row = Rect::new(info_x, y, (x + width - info_x).max(0.0), 22.0);
    let eye_btn = Rect::new(info_x, y + 2.0, 18.0, 18.0);
    let eye_hovered = app.pointer.hover == Hover::Log(LogTarget::DetailsFilterAuthor);
    draw_button(frame, p, eye_btn, content_area, eye_hovered, false);
    icons::eye(
        frame,
        center(eye_btn),
        0.75,
        if eye_hovered { p.text } else { p.text_muted },
        content_area,
    );
    hits.push((
        eye_btn.intersect(content_area),
        LogTarget::DetailsFilterAuthor,
    ));

    let style = (app.fonts.ui_small, p.text, false, None);
    let end = selectable(
        app,
        frame,
        runs,
        (eye_btn.right() + 5.0, author_row),
        &commit.author,
        style,
        content_area,
    );
    let email_room = (x + width - end - 6.0).clamp(0.0, 200.0);
    let email = truncate_end(frame, &commit.email, app.fonts.ui_small, email_room);
    let full = (email != commit.email).then_some(commit.email.as_str());
    let style = (app.fonts.ui_small, p.text_dim, false, full);
    let end = selectable(
        app,
        frame,
        runs,
        (end + 6.0, author_row),
        &email,
        style,
        content_area,
    );
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let on_date = t!("git-log-on-date", date = commit.local.as_str());
    let end = selectable(
        app,
        frame,
        runs,
        (end + 8.0, author_row),
        &on_date,
        (app.fonts.ui_small, p.text_muted, false, None),
        content_area,
    );
    let relative = display_time(commit, now);
    if is_relative_time(commit, now) {
        ui_text_small(
            frame,
            app,
            end + 8.0,
            author_row,
            &relative,
            p.text_dim,
            content_area,
        );
    }
    y += 22.0;

    // 提交号、父提交与所在分支。
    let meta_row = Rect::new(info_x, y, (x + width - info_x).max(0.0), 22.0);
    let end = ui_text_small(
        frame,
        app,
        info_x,
        meta_row,
        t!("git-log-details-commit"),
        p.text_dim,
        content_area,
    );
    let oid_x = end + 8.0;
    let hovered = app.pointer.hover == Hover::Log(LogTarget::CopyOid);
    let short_oid = short(&commit.oid);
    let oid_text_w = frame.measure(FontKind::Mono, app.fonts.ui_small, short_oid);
    let pill_h = 18.0;
    let oid_pill = Rect::new(
        oid_x,
        (y + (22.0 - pill_h) / 2.0).round(),
        oid_text_w + 26.0,
        pill_h,
    );
    let pill_fill = if hovered {
        p.selected_inactive
    } else {
        p.hover
    };
    frame.rect_clipped(oid_pill, pill_fill, 4.0, content_area);
    let oid_ink = link(p, hovered);
    let oid_end = mono(
        app,
        frame,
        (oid_x + 5.0, meta_row),
        short_oid,
        oid_ink,
        content_area,
    );
    icons::copy(
        frame,
        (oid_end + 8.0, meta_row.y + meta_row.h / 2.0),
        if hovered { oid_ink } else { p.text_muted },
        content_area,
    );
    hits.push((
        Rect::new(oid_pill.x, y, oid_pill.w, 22.0).intersect(content_area),
        LogTarget::CopyOid,
    ));
    let mut px = oid_pill.right() + 14.0;
    if !commit.parents.is_empty() {
        let label = t!("git-log-details-parents", count = commit.parents.len());
        px = ui_text_small(frame, app, px, meta_row, &label, p.text_dim, content_area) + 8.0;
        for (i, parent) in commit.parents.iter().enumerate() {
            let hovered = app.pointer.hover == Hover::Log(LogTarget::Parent(i));
            let short_p = short(parent);
            let pw = frame.measure(FontKind::Mono, app.fonts.ui_small, short_p);
            let p_pill = Rect::new(px, (y + (22.0 - pill_h) / 2.0).round(), pw + 10.0, pill_h);
            let p_fill = if hovered {
                p.selected_inactive
            } else {
                p.hover
            };
            frame.rect_clipped(p_pill, p_fill, 4.0, content_area);
            mono(
                app,
                frame,
                (px + 5.0, meta_row),
                short_p,
                link(p, hovered),
                content_area,
            );
            hits.push((
                Rect::new(p_pill.x, y, p_pill.w, 22.0).intersect(content_area),
                LogTarget::Parent(i),
            ));
            px = p_pill.right() + 6.0;
        }
    }
    y += 22.0;
    y = draw_branches(
        app,
        frame,
        details,
        (info_x, y, (x + width - info_x).max(0.0)),
        content_area,
    );

    // 完整提交正文（`.commit-body`）。
    if !body.is_empty() {
        y += 4.0;
        let body_w = (x + width - info_x).max(80.0);
        for (paragraph, lines) in body.lines().zip(&wrapped.body) {
            for (i, line) in lines.iter().enumerate() {
                let row = Rect::new(info_x, y, body_w, 20.0);
                let style = (app.fonts.ui_small, p.text_muted, i > 0, None);
                let line = &paragraph[line.clone()];
                selectable(app, frame, runs, (info_x, row), line, style, content_area);
                y += 20.0;
            }
        }
    }

    // 确保下方文件列表在大头像底部之下。
    if show_big_avatar {
        y = y.max(info_top + avatar_s + 4.0);
    }

    // 改动的文件（`.comitted-files`）。
    y += 8.0;
    frame.rect_clipped(Rect::new(x, y, width, 1.0), p.separator, 0.0, content_area);
    y += 8.0;
    let Some(details) = details else {
        return y;
    };
    if let Some(error) = &details.error {
        let row = Rect::new(x, y, width, 22.0);
        ui_text_small(frame, app, x, row, error, p.warning, content_area);
        y += 26.0;
    }
    let Some(files) = &details.files else {
        if !app.log_quiet() {
            let row = Rect::new(x, y, width, 22.0);
            let text = t!("git-log-details-loading-files");
            ui_text_small(frame, app, x, row, text, p.text_dim, content_area);
        }
        return y + 22.0;
    };
    draw_files(
        app,
        frame,
        files,
        (x, y, width),
        content_area,
        (hits, long_files),
    )
}

/// 详情里「所在分支」一项最多写几行，其余的收成「+N」。
const BRANCH_LINES: usize = 2;

/// 绘制包含该提交的分支列表。
fn draw_branches(
    app: &App,
    frame: &mut Frame<'_>,
    details: Option<&winc_editor::git::log::Details>,
    (x, mut y, width): (f32, f32, f32),
    area: Rect,
) -> f32 {
    let p = app.palette;
    let size = app.fonts.ui_small;
    let row = Rect::new(x, y, width, 20.0);
    let label = t!("git-log-details-branches");
    let start = ui_text_small(frame, app, x, row, label, p.text_dim, area) + 8.0;
    let failed = details.is_some_and(|d| app.git.log.branches_failed.contains(&d.oid));
    if failed {
        return y;
    }
    let Some(branches) = details.and_then(|d| d.branches.as_ref()) else {
        let text = t!("git-log-details-branches-loading");
        ui_text_small(frame, app, start, row, text, p.text_dim, area);
        return y + 20.0;
    };
    if branches.is_empty() {
        let text = t!("git-log-details-branches-none");
        ui_text_small(frame, app, start, row, text, p.text_muted, area);
        return y + 20.0;
    }
    let local = |name: &str| (app.git.branches.iter()).any(|b| !b.remote && b.name == name);
    let gap = frame.measure(FontKind::Ui, size, "  ");
    let (mut bx, mut line) = (start, 1);
    for (i, name) in branches.iter().enumerate() {
        let w = frame.measure(FontKind::Ui, size, name);
        if bx > start && bx + w > x + width {
            if line == BRANCH_LINES {
                let more = t!("git-log-details-branches-more", count = branches.len() - i);
                let row = Rect::new(x, y, width, 20.0);
                ui_text_small(frame, app, bx, row, &more, p.text_dim, area);
                break;
            }
            (bx, line) = (start, line + 1);
            y += 18.0;
        }
        let row = Rect::new(x, y, width, 20.0);
        let ink = if local(name) {
            p.text_muted
        } else {
            p.text_dim
        };
        let shown = truncate_end(frame, name, size, x + width - bx);
        bx = ui_text_small(frame, app, bx, row, &shown, ink, area) + gap;
    }
    y + 20.0
}

/// 绘制比较两个版本时的详情：旧的一边、新的一边、交换两侧（⇄）按钮、以及两者之间改动的文件。
fn draw_compare(
    app: &App,
    frame: &mut Frame<'_>,
    view: &LogView,
    compare: &winc_editor::git::log::Compare,
    area: Rect,
    (hits, long_files): (&mut Vec<(Rect, LogTarget)>, &mut Vec<usize>),
) -> f32 {
    let p = app.palette;
    let x = area.x + PAD;
    let width = area.w - 2.0 * PAD;
    let mut y = area.y + details_top(app.fonts.line_height) - view.details_scroll;
    let row = Rect::new(x, y, width, 24.0);
    let end = ui_text(frame, app, x, row, t!("git-compare-title"), p.text, area);
    // 紧随标题放置「交换两侧（⇄）」按钮，不与右上角固定 `Find file` / `×` 冲突。
    let swap_btn = Rect::new(end + 10.0, y + 1.0, 22.0, 22.0);
    let hovered = app.pointer.hover == Hover::Log(LogTarget::CompareSwap);
    draw_button(frame, p, swap_btn, area, hovered, false);
    icons::swap(
        frame,
        center(swap_btn),
        if hovered { p.text } else { p.text_muted },
        area,
    );
    hits.push((swap_btn.intersect(area), LogTarget::CompareSwap));
    y += 28.0;

    let sides = [
        (t!("git-compare-old"), &compare.from, &compare.from_name),
        (t!("git-compare-new"), &compare.to, &compare.to_name),
    ];
    for (label, rev, name) in sides {
        let row = Rect::new(x, y, width, 22.0);
        let end = ui_text_small(frame, app, x, row, label, p.text_dim, area) + 10.0;
        let commit = view.commits.iter().find(|c| rev_matches(c, rev));
        let shown = name.as_deref().unwrap_or(rev);
        let is_oid = matches!(shown.len(), 40 | 64) && shown.bytes().all(|b| b.is_ascii_hexdigit());
        let end = if is_oid {
            mono(app, frame, (end, row), short(shown), p.accent, area)
        } else {
            let badge = Badge {
                text: shown.to_owned(),
                fill: p.accent.alpha(p.accent.a * 0.16),
                ink: p.accent,
                mark: None,
            };
            draw_badge(app, frame, (end, row), &badge, area)
        };
        if let Some(commit) = commit {
            let clip = Rect::new(x, y, width, 22.0).intersect(area);
            let room = (x + width - end - 8.0).max(0.0);
            let subject = truncate_end(frame, &commit.subject, app.fonts.ui_small, room);
            ui_text_small(frame, app, end + 8.0, row, &subject, p.text_muted, clip);
        }
        y += 22.0;
    }
    y += 6.0;
    frame.rect_clipped(Rect::new(x, y, width, 1.0), p.separator, 0.0, area);
    y += 8.0;
    if let Some(error) = &compare.error {
        let row = Rect::new(x, y, width, 22.0);
        ui_text_small(frame, app, x, row, error, p.warning, area);
        return y + 22.0;
    }
    match &compare.files {
        Some(files) if files.is_empty() => {
            let row = Rect::new(x, y, width, 22.0);
            ui_text_small(
                frame,
                app,
                x,
                row,
                t!("git-compare-no-differences"),
                p.text_dim,
                area,
            );
            y + 22.0
        }
        Some(files) => draw_files(app, frame, files, (x, y, width), area, (hits, long_files)),
        None if app.log_quiet() => y,
        None => {
            let row = Rect::new(x, y, width, 22.0);
            ui_text_small(
                frame,
                app,
                x,
                row,
                t!("git-compare-comparing"),
                p.text_dim,
                area,
            );
            y + 22.0
        }
    }
}

/// 绘制详情底部快捷键提示条（不随内容淡入）。
pub(super) fn draw_details_footer(
    app: &App,
    frame: &mut Frame<'_>,
    view: &LogView,
    parts: &LogParts,
) {
    let p = app.palette;
    let area = parts.details;
    if area.h < 60.0
        || view.commits.is_empty()
        || (view.compare.is_none() && view.selected.is_none())
    {
        return;
    }
    let row = Rect::new(area.x, area.bottom() - 30.0, area.w, 30.0);
    frame.rect(row, p.editor_bg);
    frame.rect(Rect::new(area.x, row.y, area.w, 1.0), p.separator);
    let mut items: Vec<(String, &str)> = if app.git.log.file.is_some() {
        vec![
            ("↑↓".into(), t!("git-log-hint-select-file")),
            (t!("common-key-enter").into(), t!("git-log-hint-open-diff")),
            ("←".into(), t!("git-log-hint-return")),
        ]
    } else if view.compare.is_some() {
        vec![
            ("→".into(), t!("git-log-hint-file-list")),
            ("Esc".into(), t!("git-log-hint-exit-compare")),
            (
                t!("git-log-hint-click").into(),
                t!("git-log-hint-view-commit"),
            ),
        ]
    } else {
        vec![
            ("→".into(), t!("git-log-hint-file-list")),
            (
                t!("git-log-hint-double-click").into(),
                t!("git-log-hint-open-diff"),
            ),
            (
                t!("git-log-hint-cmd-click", key = COMMAND),
                t!("git-log-hint-compare"),
            ),
        ]
    };
    if !app.git.log.back.is_empty()
        && let Some(key) = keymap::shortcut(Action::GoBack)
    {
        items.insert(0, (key, t!("git-log-hint-go-back")));
    }
    let hints = Rect::new(area.x + PAD, row.y, area.w - 2.0 * PAD, row.h);
    draw_hints(app, frame, hints, &items, row);
}

/// 绘制一行快捷键提示。
pub(super) fn draw_hints(
    app: &App,
    frame: &mut Frame<'_>,
    row: Rect,
    items: &[(String, &str)],
    clip: Rect,
) {
    const CAP_H: f32 = 18.0;
    const CAP_PAD: f32 = 6.0;
    const GAP: f32 = 14.0;
    let p = app.palette;
    let size = app.fonts.ui_small;
    let widths: Vec<(f32, f32)> = items
        .iter()
        .map(|(key, label)| {
            let cap = frame.measure(FontKind::Ui, size, key) + 2.0 * CAP_PAD;
            (cap, frame.measure(FontKind::Ui, size, label))
        })
        .collect();
    let mut count = items.len();
    let total = |n: usize| -> f32 {
        widths[..n].iter().map(|(c, l)| c + 5.0 + l).sum::<f32>() + GAP * n.saturating_sub(1) as f32
    };
    while count > 0 && total(count) > row.w {
        count -= 1;
    }
    let mut x = row.x;
    for ((key, label), (cap_w, _)) in items.iter().zip(&widths).take(count) {
        let cap = Rect::new(x, (row.y + (row.h - CAP_H) / 2.0).round(), *cap_w, CAP_H);
        frame.rect_clipped(cap, p.selected_inactive, 4.0, clip);
        caps_label(
            frame,
            app,
            size,
            (cap.x + CAP_PAD, cap),
            key,
            p.text_muted,
            clip,
        );
        x = ui_text_small(frame, app, cap.right() + 5.0, row, label, p.text_dim, clip) + GAP;
    }
}

/// 新选的提交的详情还没取齐、又刚开始取时继续显示上一个提交和它的详情。
fn held_details<'a>(
    app: &App,
    view: &'a LogView,
    selected: &Commit,
) -> Option<(&'a Commit, &'a winc_editor::git::log::Details)> {
    let current = view.details.as_ref().filter(|d| d.oid == selected.oid);
    let ready = current.is_some_and(|d| d.files.is_some() || d.error.is_some());
    view.previous
        .as_ref()
        .filter(|_| !ready && app.log_quiet())
        .and_then(|d| Some((view.commits.iter().find(|c| c.oid == d.oid)?, d)))
}

/// 详情一栏这一帧画的是哪个提交：比较两个版本时、没选中时为 `None`。
pub(super) fn shown_oid(app: &App, view: &LogView) -> Option<String> {
    if view.compare.is_some() {
        return None;
    }
    let selected = view.selected.and_then(|i| view.commits.get(i))?;
    let commit = held_details(app, view, selected).map_or(selected, |(c, _)| c);
    Some(commit.oid.clone())
}

/// 在详情里画一行可以选中复制的字，记进 `runs`。
fn selectable(
    app: &App,
    frame: &mut Frame<'_>,
    runs: &mut Vec<TextRun>,
    (x, row): (f32, Rect),
    text: &str,
    (size, color, soft, full): (f32, Color, bool, Option<&str>),
    clip: Rect,
) -> f32 {
    let mut edges = Vec::with_capacity(text.len());
    let (mut right, mut buf) = (x, [0u8; 4]);
    for c in text.chars() {
        right += frame.measure(FontKind::Ui, size, c.encode_utf8(&mut buf));
        edges.push(right);
    }
    if let Some((from, to)) = app.log_selected_chars(runs.len(), edges.len()) {
        let left = if from == 0 { x } else { edges[from - 1] };
        let band = Rect::new(left, row.y + 1.0, edges[to - 1] - left, row.h - 2.0);
        frame.rect_clipped(band, app.palette.selection, 2.0, clip);
    }
    runs.push(TextRun {
        row,
        x,
        text: text.to_owned(),
        edges,
        full: full.map(str::to_owned),
        soft,
    });
    if size == app.fonts.ui {
        ui_text(frame, app, x, row, text, color, clip)
    } else {
        ui_text_small(frame, app, x, row, text, color, clip)
    }
}

/// 提交 `commit` 是不是版本 `rev`：提交号相同，或者 `rev` 是指着它的分支、标签、`HEAD`。
pub(super) fn rev_matches(commit: &Commit, rev: &str) -> bool {
    commit.oid == rev
        || commit.refs.iter().any(|r| match r {
            Ref::Head(name) | Ref::Branch(name) | Ref::Remote(name) | Ref::Tag(name) => name == rev,
            Ref::Detached => rev == "HEAD",
        })
        || (rev == "HEAD" && commit.refs.iter().any(|r| matches!(r, Ref::Head(_))))
}
