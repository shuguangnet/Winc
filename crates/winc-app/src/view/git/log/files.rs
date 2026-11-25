//! 详情栏改动文件列表（1:1 还原 `gitHistoryVSCode` `components/LogView/Commit/FileEntry/index.tsx`）。
//!
//! 每一行 `.diff-row` 包含：
//! - 左侧：5 格增删色块（`.diff-stats`）+ 状态图标（`M/A/D/R`）+ 文件图标 + 完整相对路径
//!   （当前文件历史高亮为 `#91b748` `.file-name-active`）；
//! - 右侧：底色操作条（`.file-action`），始终显示 4 个显式蓝色链接按钮：
//!   `[eye] View`、`[compare] Workspace`、`[compare] Previous`、`[history] History`
//!   （窄屏左右分栏下自动紧凑为图标按钮，避免挤压文件路径）。
//! - 详情右上角始终显示内联 `Find file` 输入框（[`draw_inline_file_filter`]）。

use super::*;
use crate::app::git::log::LogFileRow;

/// 1:1 对齐 `FileEntry/index.tsx`：`TotalDiffBlocks = 5`。
const TOTAL_DIFF_BLOCKS: usize = 5;

/// 绘制改动的文件列表（`.comitted-files`）。
pub(super) fn draw_files(
    app: &App,
    frame: &mut Frame<'_>,
    files: &[winc_editor::git::CommitFile],
    (x, mut y, width): (f32, f32, f32),
    area: Rect,
    (hits, long_files): (&mut Vec<(Rect, LogTarget)>, &mut Vec<usize>),
) -> f32 {
    let p = app.palette;
    let rows = app.log_file_rows_of(files);
    let shown: Vec<usize> = rows
        .iter()
        .filter_map(|r| match r {
            LogFileRow::File(i) => Some(*i),
            LogFileRow::Dir(_) => None,
        })
        .collect();
    let (added, removed) = shown
        .iter()
        .filter_map(|i| files[*i].lines)
        .fold((0, 0), |(a, d), (x, y)| (a + x, d + y));

    // 摘要标题行：「N 个文件 +A −D」。
    let row = Rect::new(x, y, width, 22.0);
    let title = match shown.len() == files.len() {
        true => t!("git-log-files-changed", count = files.len()),
        false => t!(
            "git-log-files-changed-of",
            shown = shown.len(),
            total = files.len()
        ),
    };
    let end = ui_text_small(frame, app, x, row, &title, p.text_muted, area);
    let end = ui_text_small(
        frame,
        app,
        end + 10.0,
        row,
        &format!("+{added}"),
        p.git_added,
        area,
    );
    ui_text_small(
        frame,
        app,
        end + 6.0,
        row,
        &format!("−{removed}"),
        p.git_deleted,
        area,
    );
    y += 24.0;

    // 「查找文件」把文件全筛掉了：写一行提示，点它清掉键入的文字。
    if rows.is_empty() && !files.is_empty() {
        let row = Rect::new(x, y, width, 24.0);
        if app.pointer.hover == Hover::Log(LogTarget::ClearFileFilter) {
            frame.rounded(row, p.hover, RADIUS_ROW);
        }
        ui_text_small(
            frame,
            app,
            x,
            row,
            t!("git-log-files-no-match"),
            p.text_dim,
            area,
        );
        hits.push((row.intersect(area), LogTarget::ClearFileFilter));
        return row.bottom();
    }

    let flat = app.git.commit.flat();
    let root = app.git.root.as_deref();
    let history_file = app
        .editor
        .active()
        .and_then(|d| d.log.as_deref()?.query.path.as_deref());
    let mut dir_counts: Vec<usize> = Vec::new();
    for entry in &rows {
        match entry {
            LogFileRow::Dir(_) => dir_counts.push(0),
            LogFileRow::File(_) => {
                if let Some(last) = dir_counts.last_mut() {
                    *last += 1;
                }
            }
        }
    }
    let mut dir_idx = 0usize;
    let mut current_dir_collapsed = false;
    let mut current_dir_has_header = false;
    // 宽度充足（≥ 520px，上下分栏模式永远满足）时显示带文字的 4 项 `FileEntry` 操作条；
    // 窄屏左右分栏时紧凑为纯图标操作条。
    let labeled_actions = width >= 500.0;

    for entry in rows {
        match entry {
            LogFileRow::Dir(dir) => {
                let idx = dir_idx;
                dir_idx += 1;
                current_dir_has_header = true;
                current_dir_collapsed = app.git.log.collapsed_dirs.contains(&dir);
                let row = Rect::new(x, y, width, FILE_ROW);
                y += FILE_ROW;
                if row.bottom() < area.y || row.y > area.bottom() {
                    continue;
                }
                let hovered = app.pointer.hover == Hover::Log(LogTarget::FileDir(idx));
                if hovered {
                    frame.rect_clipped(row, p.hover, RADIUS_ROW, area);
                }
                let cy = row.y + row.h / 2.0;
                let angle = if current_dir_collapsed {
                    0.0
                } else {
                    FRAC_PI_2
                };
                icons::chevron_turned(frame, (x + 4.0, cy), 6.0, angle, p.text_dim, area);
                let folder_x = x + 14.0;
                icons::folder(
                    frame,
                    (folder_x, (cy - 6.0).round()),
                    !current_dir_collapsed,
                    p.folder,
                    area,
                );
                let count = dir_counts.get(idx).copied().unwrap_or(0);
                let count_str = count.to_string();
                let count_w = frame.measure(FontKind::Ui, app.fonts.ui_small, &count_str) + 12.0;
                let pill_h = 16.0;
                let pill = Rect::new(
                    row.right() - 6.0 - count_w,
                    (row.y + (row.h - pill_h) / 2.0).round(),
                    count_w,
                    pill_h,
                );
                controls::count_pill(app, frame, pill, &count_str, 6.0, area);
                let left = folder_x + icons::FOLDER_WIDTH + 6.0;
                let room = (pill.x - 8.0 - left).max(0.0);
                let shown_dir = truncate_end(frame, &dir, app.fonts.ui_small, room);
                ui_text_small(frame, app, left, row, &shown_dir, p.text_muted, area);
                hits.push((row.intersect(area), LogTarget::FileDir(idx)));
                continue;
            }
            LogFileRow::File(_) if current_dir_collapsed => continue,
            LogFileRow::File(i) => {
                let row = Rect::new(x, y, width, FILE_ROW);
                y += FILE_ROW;
                if row.bottom() < area.y || row.y > area.bottom() {
                    continue;
                }
                let file = &files[i];
                let hovered = matches!(
                    app.pointer.hover,
                    Hover::Log(
                        LogTarget::File(h)
                            | LogTarget::FileView(h)
                            | LogTarget::FileCompareLocal(h)
                            | LogTarget::FileComparePrevious(h)
                            | LogTarget::FileHistory(h)
                    ) if h == i
                );
                let chosen = app.git.log.file == Some(i);
                if let Some(color) = row_color(p, chosen, app.focus == Focus::Editor, hovered) {
                    frame.rect_clipped(row, color, 2.0, area);
                }
                // 对齐 `main.css:592-594`：每个 `.diff-row` 底部画 1px 分隔线。
                frame.rect_clipped(
                    Rect::new(row.x, row.bottom() - 1.0, row.w, 1.0),
                    p.separator,
                    0.0,
                    area,
                );
                if !flat && current_dir_has_header {
                    controls::indent_guide(
                        frame,
                        p,
                        x + 4.0,
                        row.y,
                        row.h,
                        chosen || hovered,
                        area,
                    );
                }

                // 右侧始终可见的 `.file-action` 操作条（1:1 `FileEntry/index.tsx:91-134`）：
                // `[eye] View` / `[compare] Workspace` / `[compare] Previous` / `[history] History`。
                let action_left = draw_file_actions(
                    app,
                    frame,
                    i,
                    (row.right() - 2.0, row, labeled_actions),
                    area,
                    hits,
                );

                // 左侧依次：5 格 `.diff-stats` 色块 + 状态字母 + 文件图标 + 相对路径。
                let mut cx = if flat {
                    x + 4.0
                } else {
                    x + crate::theme::TREE_INDENT
                };
                let cy = row.y + row.h / 2.0;
                let left_clip =
                    Rect::new(row.x, row.y, (action_left - row.x - 8.0).max(0.0), row.h)
                        .intersect(area);

                // 1. 5 格 `.diff-stats` 色块（1:1 `FileEntry/index.tsx:49-66`）。
                cx = draw_diff_blocks(app, frame, file.lines, (cx, cy), left_clip) + 8.0;

                // 2. 状态字母（M/A/D/R）。
                let status_color = git_color(p, file.change);
                let letter = file.change.letter().to_string();
                let lw = frame.measure(FontKind::Ui, app.fonts.ui_small, &letter);
                let status_slot = 13.0;
                ui_text_small(
                    frame,
                    app,
                    cx + (status_slot - lw) / 2.0,
                    row,
                    &letter,
                    status_color,
                    left_clip,
                );
                cx += status_slot + 3.0;

                // 3. 文件图标 + 路径（平铺模式下显示完整相对路径，对齐 `FileEntry/index.tsx:88`；
                //    当前文件历史高亮 `#91b748` `.file-name-active`）。
                let name = winc_editor::display_name(&file.path).into_owned();
                let icon_end = draw_file_icon(frame, p, (cx, cy), &name, left_clip);
                let rel_path = root
                    .and_then(|r| file.path.strip_prefix(r).ok())
                    .map(|rel| rel.to_string_lossy().into_owned())
                    .unwrap_or_else(|| name.clone());
                let shown_text = match (&file.from, file.change) {
                    (Some(from), Change::Renamed) => {
                        let old_rel = root
                            .and_then(|r| from.strip_prefix(r).ok())
                            .map(|rel| rel.to_string_lossy().into_owned())
                            .unwrap_or_else(|| winc_editor::display_name(from).into_owned());
                        format!("{old_rel} → {rel_path}")
                    }
                    _ if !flat => name,
                    _ => rel_path,
                };
                let active_history = history_file.is_some_and(|h| h == file.path.as_path());
                let path_color = if active_history {
                    Color::hex(0x91B748)
                } else {
                    p.text
                };
                let text_x = icon_end + 6.0;
                let room = (left_clip.right() - text_x - 4.0).max(0.0);
                let full_w = frame.measure(FontKind::Ui, app.fonts.ui_small, &shown_text);
                if full_w > room || file.from.is_some() {
                    long_files.push(i);
                }
                let shown = truncate_end(frame, &shown_text, app.fonts.ui_small, room);
                ui_text_small(frame, app, text_x, row, &shown, path_color, left_clip);

                hits.push((row.intersect(area), LogTarget::File(i)));
            }
        }
    }
    y
}

/// 绘制 1:1 `gitHistoryVSCode` 的 5 格 `.diff-stats` 色块（`FileEntry/index.tsx:49-66`）。
///
/// 返回色块组右侧的 x 坐标。
fn draw_diff_blocks(
    app: &App,
    frame: &mut Frame<'_>,
    lines: Option<(usize, usize)>,
    (x, cy): (f32, f32),
    clip: Rect,
) -> f32 {
    let p = app.palette;
    let block_s = 8.0;
    let step = 9.0;
    let (added_blocks, deleted_blocks) = match lines {
        Some((a, d)) => {
            let total = a + d;
            if total == 0 {
                (0, 0)
            } else if total > TOTAL_DIFF_BLOCKS {
                let ab = ((TOTAL_DIFF_BLOCKS * a) as f32 / total as f32).ceil() as usize;
                let ab = ab.min(TOTAL_DIFF_BLOCKS);
                (ab, TOTAL_DIFF_BLOCKS - ab)
            } else {
                (
                    a.min(TOTAL_DIFF_BLOCKS),
                    d.min(TOTAL_DIFF_BLOCKS - a.min(TOTAL_DIFF_BLOCKS)),
                )
            }
        }
        None => (0, 0),
    };
    for k in 0..TOTAL_DIFF_BLOCKS {
        let color = if k < added_blocks {
            p.git_added
        } else if k < added_blocks + deleted_blocks {
            p.git_deleted
        } else {
            p.separator
        };
        let rect = Rect::new(
            x + k as f32 * step,
            (cy - block_s / 2.0).round(),
            block_s,
            block_s,
        );
        frame.rect_clipped(rect, color, 1.0, clip);
    }
    x + TOTAL_DIFF_BLOCKS as f32 * step - 1.0
}

/// 在文件行右端绘制 1:1 `gitHistoryVSCode` `.file-action` 操作条：
/// `[eye] View  [compare] Workspace  [compare] Previous  [history] History`。
///
/// 返回操作条的最左 x 坐标。
fn draw_file_actions(
    app: &App,
    frame: &mut Frame<'_>,
    i: usize,
    (right_x, row, labeled): (f32, Rect, bool),
    clip: Rect,
    hits: &mut Vec<(Rect, LogTarget)>,
) -> f32 {
    let p = app.palette;
    let size = app.fonts.ui_small * 0.92;
    let strip_h = 20.0;
    let y = (row.y + (row.h - strip_h) / 2.0).round();
    let actions: [(LogTarget, &str); 4] = [
        (LogTarget::FileView(i), t!("git-log-file-btn-view")),
        (
            LogTarget::FileCompareLocal(i),
            t!("git-log-file-btn-workspace"),
        ),
        (
            LogTarget::FileComparePrevious(i),
            t!("git-log-file-btn-previous"),
        ),
        (LogTarget::FileHistory(i), t!("git-log-file-btn-history")),
    ];

    if !labeled {
        // 窄屏紧凑图标条。
        let btn_s = 20.0;
        let gap = 2.0;
        let total_w = 6.0 + actions.len() as f32 * btn_s + (actions.len() - 1) as f32 * gap + 4.0;
        let x0 = right_x - total_w;
        frame.rect_clipped(Rect::new(x0, y, total_w, strip_h), p.hover, 3.0, clip);
        for (k, (target, _)) in actions.into_iter().enumerate() {
            let btn = Rect::new(x0 + 3.0 + k as f32 * (btn_s + gap), y, btn_s, strip_h);
            let on = app.pointer.hover == Hover::Log(target);
            if on {
                frame.rect_clipped(btn, p.selected_inactive, 2.0, clip);
            }
            let ink = if on { p.text } else { p.accent };
            draw_file_action_icon(frame, target, center(btn), ink, clip);
            hits.push((btn.intersect(clip), target));
        }
        return x0;
    }

    // 完整带图标+文字操作条（1:1 `FileEntry/index.tsx:91-134`）。
    let icon_w = 15.0;
    let item_gap = 12.0;
    let mut item_ws = [0.0f32; 4];
    let mut total_w = 14.0;
    for (k, (_, label)) in actions.iter().enumerate() {
        let w = icon_w + 4.0 + frame.measure(FontKind::Ui, size, label);
        item_ws[k] = w;
        total_w += w;
        if k > 0 {
            total_w += item_gap;
        }
    }
    let x0 = right_x - total_w;
    let strip = Rect::new(x0, y, total_w, strip_h);
    frame.rect_clipped(strip, p.hover, 3.0, clip);

    let (ua, ud) = (app.metrics.ui.ascent, app.metrics.ui.descent);
    let uscale = size / app.fonts.ui;
    let ubase = baseline_in(strip, ua * uscale, ud * uscale);
    let mut cx = x0 + 7.0;
    for (k, (target, label)) in actions.into_iter().enumerate() {
        let w = item_ws[k];
        let btn = Rect::new(cx - 3.0, y, w + 6.0, strip_h);
        let on = app.pointer.hover == Hover::Log(target);
        if on {
            frame.rect_clipped(btn, p.selected_inactive, 2.0, clip);
        }
        let ink = if on { p.text } else { p.accent };
        draw_file_action_icon(frame, target, (cx + 6.5, y + strip_h / 2.0), ink, clip);
        frame.text(
            FontKind::Ui,
            size,
            cx + icon_w + 3.0,
            ubase,
            label,
            ink,
            clip,
        );
        hits.push((btn.intersect(clip), target));
        cx += w + item_gap;
    }
    x0
}

fn draw_file_action_icon(
    frame: &mut Frame<'_>,
    target: LogTarget,
    pos: (f32, f32),
    ink: Color,
    clip: Rect,
) {
    match target {
        LogTarget::FileView(_) => icons::eye(frame, pos, 0.72, ink, clip),
        LogTarget::FileCompareLocal(_) | LogTarget::FileComparePrevious(_) => {
            icons::compare(frame, pos, ink, clip);
        }
        _ => icons::history(frame, pos, ink, clip),
    }
}

/// 绘制详情右上角始终可见的内联「Find file」输入框（1:1 对齐 `Commit/index.tsx:117-127`）。
pub(super) fn draw_inline_file_filter(
    app: &mut App,
    frame: &mut Frame<'_>,
    parts: &LogParts,
    hits: &mut Vec<(Rect, LogTarget)>,
) {
    let p = app.palette;
    let rect = inline_file_filter_rect(app, parts);
    let focused = app.focus == Focus::LogFileFilter;
    field_box(app, frame, rect, p.editor_bg, focused, false);
    icons::magnifier(
        frame,
        (rect.x + 12.0, rect.y + rect.h / 2.0),
        p.text_dim,
        rect,
    );
    let inside = Rect::new(rect.x + 24.0, rect.y, (rect.w - 28.0).max(0.0), rect.h);
    let mut field = std::mem::take(&mut app.git.log.file_filter);
    let caret = draw_bar_field(
        app,
        frame,
        inside,
        &mut field,
        t!("git-log-find-file"),
        focused,
    );
    app.git.log.file_filter = field;
    if focused {
        app.ime.cursor = caret;
    }
    hits.push((rect, LogTarget::FileFilter));
}
