//! 悬停提示框（Tooltip）的绘制与文本折行。

use super::*;

/// 绘制当前悬停目标的提示框：停留超过 `TOOLTIP_DELAY` 后显示在目标下方（空间不足时改在上方）。
pub(super) fn draw_tooltip(app: &App, frame: &mut Frame<'_>) {
    // 停留时间不足时直接返回，不拼接提示文字，也不查找命中框。
    let rested = app
        .overlays
        .tooltip
        .since
        .is_some_and(|since| since.elapsed() >= TOOLTIP_DELAY);
    if !rested {
        return;
    }
    let (button, label): (Rect, String) = match workspace::panel::tooltip(app) {
        Some(tip) => tip,
        None => match app.pointer.hover {
            Hover::Search(button) => (
                SearchParts::new(app.layout.search).rect(button),
                button.label(),
            ),
            Hover::TreeTool(tool) => match app.hits.tree_tools.iter().find(|(_, t)| *t == tool) {
                Some(&(button, _)) => (button, tool.label().to_owned()),
                None => return,
            },
            Hover::TreeMore => match app.hits.tree_more {
                Some(button) => (button, t!("workspace-tip-more-actions").to_owned()),
                None => return,
            },
            Hover::SidebarButton => (
                app.layout.sidebar_button,
                crate::layout::with_key(t!("cmd-toggle-sidebar"), Some(Action::ToggleSidebar)),
            ),
            Hover::CommandCenter => (
                app.layout.command_center,
                crate::layout::with_key(t!("cmd-quick-open"), Some(Action::QuickOpen)),
            ),
            Hover::PanelButton => (
                app.layout.panel_button,
                crate::layout::with_key(t!("cmd-toggle-terminal"), Some(Action::ToggleTerminal)),
            ),
            Hover::SplitButton => (
                app.layout.split_button,
                crate::layout::with_key(t!("cmd-split-right"), Some(Action::SplitRight)),
            ),
            Hover::ActivitySettings => (
                app.layout.activity_settings,
                crate::layout::with_key(t!("cmd-settings"), Some(Action::Settings)),
            ),
            Hover::Browser => (app.layout.browser, t!("preview-browser-open").to_owned()),
            Hover::Preview(mode) => {
                use winc_editor::PreviewMode::*;
                let [source, split, preview] = crate::layout::preview_segments(app.layout.preview);
                // 「预览」对应 ⇧⌘V（在源代码与预览之间切换）。
                match mode {
                    Source => (source, t!("preview-mode-source").to_owned()),
                    Split => (split, t!("preview-mode-split").to_owned()),
                    Preview => (
                        preview,
                        crate::layout::with_key(
                            t!("preview-mode-preview"),
                            Some(Action::TogglePreview),
                        ),
                    ),
                }
            }
            // 日志列表中的作者头像：提示作者名和邮箱。
            Hover::Log(crate::app::git::log::LogTarget::Avatar(i)) => {
                let hit = app
                    .hits
                    .log
                    .iter()
                    .find(|(_, t)| *t == crate::app::git::log::LogTarget::Avatar(i));
                let commit = app
                    .editor
                    .active()
                    .and_then(|d| d.log.as_deref())
                    .and_then(|v| v.commits.get(i));
                match (hit, commit) {
                    (Some(&(rect, _)), Some(c)) => (rect, format!("{} <{}>", c.author, c.email)),
                    _ => return,
                }
            }
            // 提交图中线段的目标：点击后跳转到的提交。
            Hover::Log(crate::app::git::log::LogTarget::Line(row, line)) => {
                let target = crate::app::git::log::LogTarget::Line(row, line);
                if !app.hits.log.iter().any(|(_, t)| *t == target) {
                    return;
                }
                // 线段的命中框很小，提示放在鼠标下方，不按命中框居中。
                let (mx, my) = app.mouse();
                let rect = Rect::new(mx, my + 6.0, 0.0, 0.0);
                let commit = app.log_line_target(row, line).and_then(|j| {
                    app.editor
                        .active()
                        .and_then(|d| d.log.as_deref())
                        .and_then(|v| v.commits.get(j))
                });
                let label = match commit {
                    Some(c) => t!(
                        "workspace-tip-log-go-to",
                        commit = crate::app::git::log::short(&c.oid),
                        subject = c.subject.as_str()
                    ),
                    None => t!("workspace-tip-log-target-not-loaded").to_owned(),
                };
                (rect, label)
            }
            // 日志分支面板中截短了的分支名：提示全名。
            Hover::Log(crate::app::git::log::LogTarget::BranchRow(i))
                if app.git.log.long_branches.contains(&i) =>
            {
                use crate::app::git::log::{BranchRow, LogTarget};
                let hit = app
                    .hits
                    .log
                    .iter()
                    .find(|(_, t)| *t == LogTarget::BranchRow(i));
                let row = app.log_branch_rows().into_iter().nth(i);
                match (hit, row) {
                    (Some(&(rect, _)), Some(BranchRow::Branch { index, .. })) => {
                        let branch = app.git.branches.get(index);
                        (rect, branch.map_or_else(String::new, |b| b.name.clone()))
                    }
                    _ => return,
                }
            }
            // 日志详情中目录被截短或文件被改名的条目：显示完整路径。
            Hover::Log(crate::app::git::log::LogTarget::File(i))
                if app.git.log.long_files.contains(&i) =>
            {
                let hit = app
                    .hits
                    .log
                    .iter()
                    .find(|(_, t)| *t == crate::app::git::log::LogTarget::File(i));
                match (hit, app.log_file_tip(i)) {
                    (Some(&(rect, _)), Some(tip)) => (rect, tip),
                    _ => return,
                }
            }
            // 日志详情栏及标题栏按钮（复制提交号、刷新、树形/列表布局切换、查找文件、文件行内快捷按钮）。
            Hover::Log(
                target @ (crate::app::git::log::LogTarget::CopyOid
                | crate::app::git::log::LogTarget::Refresh
                | crate::app::git::log::LogTarget::FilesLayout
                | crate::app::git::log::LogTarget::FindFiles
                | crate::app::git::log::LogTarget::FileOpen(_)
                | crate::app::git::log::LogTarget::FileCompareLocal(_)
                | crate::app::git::log::LogTarget::FileHistory(_)),
            ) => {
                use crate::app::git::log::LogTarget;
                let Some(&(rect, _)) = app.hits.log.iter().find(|(_, t)| *t == target) else {
                    return;
                };
                let label = match target {
                    LogTarget::CopyOid => t!("menu-log-copy-oid"),
                    LogTarget::Refresh => t!("workspace-tip-git-refresh"),
                    LogTarget::FilesLayout if app.git.commit.flat() => {
                        t!("workspace-tip-show-as-tree")
                    }
                    LogTarget::FilesLayout => t!("workspace-tip-show-as-list"),
                    LogTarget::FindFiles => t!("workspace-find-placeholder"),
                    LogTarget::FileOpen(_) => t!("workspace-tip-open-file"),
                    LogTarget::FileCompareLocal(_) => t!("menu-log-compare-with-local"),
                    LogTarget::FileHistory(_) => t!("menu-git-file-history"),
                    _ => return,
                };
                (rect, label.to_owned())
            }
            // 日志列表中延伸到右端仍放不下的提交说明：提示整句。
            Hover::Log(crate::app::git::log::LogTarget::Row(i))
                if app.git.log.clipped == Some(i) =>
            {
                let hit = app
                    .hits
                    .log
                    .iter()
                    .find(|(_, t)| *t == crate::app::git::log::LogTarget::Row(i));
                let commit = app
                    .editor
                    .active()
                    .and_then(|d| d.log.as_deref())
                    .and_then(|v| v.commits.get(i));
                match (hit, commit) {
                    (Some(&(rect, _)), Some(c)) => (rect, c.subject.clone()),
                    _ => return,
                }
            }
            _ => return,
        },
    };
    let label = label.as_str();

    let p = app.palette;
    // 提示分「名称   快捷键」两栏，快捷键以淡色显示在右侧，两栏间距至少 16 逻辑像素。含 `\n` 时
    // 第一段为正文，其余各段为淡色说明（如标签的文件名和所在目录）。
    let (name, key) = match label.split_once(crate::layout::TIP_KEY) {
        Some((name, key)) => (name, Some(key)),
        None => (label, None),
    };
    let size = app.fonts.ui_small;
    let key_w = key.map_or(0.0, |k| 16.0 + frame.measure(FontKind::Ui, size, k));
    // 提示宽度不超过 `TOOLTIP_MAX_WIDTH`，也不超出窗口（左右各留 4 逻辑像素边距）；
    // 文字区再减去左右各 10 逻辑像素的内边距。
    let max_width = TOOLTIP_MAX_WIDTH.min(app.layout.window.w - 8.0);
    let room = max_width - 20.0;
    let lines = tip_lines(name, room, key_w, |piece| {
        frame.measure(FontKind::Ui, size, piece)
    });
    let name_w = lines
        .iter()
        .map(|(line, _)| frame.measure(FontKind::Ui, size, line))
        .fold(0.0, f32::max);
    let width = (name_w + key_w + 20.0).min(max_width);
    // 单行高 26 逻辑像素，每多一行增加 18。
    const LINE: f32 = 18.0;
    let height = 8.0 + LINE * lines.len() as f32;
    let rect = if matches!(app.pointer.hover, Hover::RulerChange(_)) {
        // 概览标尺紧靠窗口右边缘：提示放在标尺左侧、与鼠标同高，不遮挡鼠标所指的标记。
        Rect::new(
            button.x - 8.0 - width,
            button.y + button.h / 2.0 - height / 2.0,
            width,
            height,
        )
    } else {
        let below = button.bottom() + 6.0;
        let y = if below + height > app.layout.window.bottom() {
            button.y - 6.0 - height
        } else {
            below
        };
        Rect::new(
            (button.x + (button.w - width) / 2.0)
                .clamp(4.0, (app.layout.window.right() - width - 4.0).max(4.0)),
            y,
            width,
            height,
        )
    };
    draw_card(frame, p, rect, RADIUS_ROW, p.popup_bg);
    for (i, &(line, body)) in lines.iter().enumerate() {
        let row = Rect::new(rect.x, rect.y + 4.0 + LINE * i as f32, rect.w, LINE);
        let color = if body { p.text } else { p.text_dim };
        ui_text_small(frame, app, rect.x + 10.0, row, line, color, rect);
    }
    if let Some(key) = key {
        let w = frame.measure(FontKind::Ui, size, key);
        let row = Rect::new(rect.x, rect.y + 4.0, rect.w, LINE);
        ui_text_small(
            frame,
            app,
            rect.right() - 10.0 - w,
            row,
            key,
            p.text_dim,
            rect,
        );
    }
}

/// 把提示正文 `name` 按宽度 `room`（逻辑像素）折行，返回各行及其是否属于正文段。
///
/// `name` 以 `\n` 分段，第一段为正文，其余为说明；正文折出的各行都属于正文段。快捷键只占
/// 第一行，因此第一段的宽度再减去 `key_w`。`measure` 返回一段文字的宽度（逻辑像素）。
pub(super) fn tip_lines(
    name: &str,
    room: f32,
    key_w: f32,
    mut measure: impl FnMut(&str) -> f32,
) -> Vec<(&str, bool)> {
    let mut lines = Vec::new();
    for (i, part) in name.split('\n').enumerate() {
        let width = (room - if i == 0 { key_w } else { 0.0 }).max(1.0);
        let wrapped = crate::layout::wrap_text(part, width, &mut measure);
        lines.extend(wrapped.into_iter().map(|line| (line, i == 0)));
    }
    lines
}
