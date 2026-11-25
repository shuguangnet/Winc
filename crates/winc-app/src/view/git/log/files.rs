//! 详情栏里改动的文件和增删的行数。
//!
//! 平时由 [`draw_files`] 在详情一栏里逐个文件画出，每个文件一行，点击打开差异；可以按目录
//! 分组（见 [`log_file_rows`]）。截短的路径记进 `long_files`，鼠标悬停时提示完整路径。
//!
//! 标题行右端的放大镜打开「查找文件」：详情底部的搜索框（[`draw_file_filter`]）按路径筛选
//! 列出的文件；一个文件也不剩时写一行提示，点它清掉键入的文字。

use super::*;
use crate::app::git::log::LogFileRow;

/// 绘制改动的文件：标题行「N 个文件 +A −D」，下面一个文件一行（图标、按变化着色的名字、
/// 淡色的目录、右端的增删统计），点击后打开差异。
///
/// 从 `(x, y)` 开始画，宽 `width`，返回画完后的 y 坐标；坐标都是逻辑像素。目录写不下而截短、
/// 或改过名的文件记进 `long_files`：悬停时提示完整的路径。
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
    // 「查找文件」筛掉了文件时只统计留下来的，标题行写成「留下来的 / 全部」。
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
    // 标题行高 22 逻辑像素（经验值），下面留 4 逻辑像素再开始画文件。
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
    // 标题行右端：平铺与按目录分组之间切换，图标是切换以后的样子（与提交视图一起切换）。
    let flat = app.git.commit.flat();
    let button = Rect::new(x + width - 24.0, y - 1.0, 24.0, 24.0);
    let hovered = app.pointer.hover == Hover::Log(LogTarget::FilesLayout);
    draw_button(frame, p, button, area, hovered, false);
    let ink = if hovered { p.text } else { p.text_muted };
    match flat {
        true => icons::tree_view(frame, center(button), ink, area),
        false => icons::list_view(frame, center(button), ink, area),
    }
    hits.push((button.intersect(area), LogTarget::FilesLayout));
    // 它左边是「查找文件」：打开时详情底部出现搜索框，按路径筛选文件。
    let find = Rect::new(button.x - 26.0, button.y, 24.0, 24.0);
    let hovered = app.pointer.hover == Hover::Log(LogTarget::FindFiles);
    let open = app.git.log.find_files;
    draw_button(frame, p, find, area, hovered, open);
    let ink = match (open, hovered) {
        (true, _) => p.accent,
        (false, true) => p.text,
        (false, false) => p.text_muted,
    };
    icons::magnifier(frame, center(find), ink, area);
    hits.push((find.intersect(area), LogTarget::FindFiles));
    y += 26.0;

    // 「查找文件」把文件全筛掉了：在标题下面写一行，点它清掉键入的文字。
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

    let root = app.git.root.as_deref();
    // 预先统计按目录分组时每个目录标题下包含的文件数。
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
    for entry in rows {
        match entry {
            LogFileRow::Dir(dir) => {
                let idx = dir_idx;
                dir_idx += 1;
                current_dir_has_header = true;
                current_dir_collapsed = app.git.log.collapsed_dirs.contains(&dir);
                let row = Rect::new(area.x + 6.0, y, area.w - 12.0, FILE_ROW);
                y += FILE_ROW;
                if row.bottom() < area.y || row.y > area.bottom() {
                    continue;
                }
                let hovered = app.pointer.hover == Hover::Log(LogTarget::FileDir(idx));
                if hovered {
                    frame.rect_clipped(row, p.hover, RADIUS_ROW, area);
                }
                // 目录标题：折叠箭头、打开/收起状态的文件夹图标、相对路径，右端显示该目录下的改动文件数。
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
                    row.right() - 8.0 - count_w,
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
            LogFileRow::File(_) if current_dir_collapsed => {
                continue;
            }
            LogFileRow::File(i) => {
                let row = Rect::new(area.x + 6.0, y, area.w - 12.0, FILE_ROW);
                y += FILE_ROW;
                if row.bottom() < area.y || row.y > area.bottom() {
                    continue;
                }
                let file = &files[i];
                let hovered = matches!(
                    app.pointer.hover,
                    Hover::Log(
                        LogTarget::File(h)
                            | LogTarget::FileOpen(h)
                            | LogTarget::FileCompareLocal(h)
                            | LogTarget::FileHistory(h)
                    ) if h == i
                );
                // 键盘选中的文件（按 `→` 进入文件列表以后）铺选中的底色。
                let chosen = app.git.log.file == Some(i);
                if let Some(color) = row_color(p, chosen, app.focus == Focus::Editor, hovered) {
                    frame.rect_clipped(row, color, RADIUS_ROW, area);
                }
                // 按目录分组且有上级目录标题时，在目录折叠箭头正下方画一条缩进参考线。
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
                let file_x = if flat {
                    x
                } else {
                    x + crate::theme::TREE_INDENT
                };
                let cy = row.y + row.h / 2.0;
                let name = winc_editor::display_name(&file.path).into_owned();
                // 增删统计占行右端 96 逻辑像素（经验值）。
                let stat_w = 96.0;
                let clip = Rect::new(row.x, row.y, row.w - stat_w, row.h).intersect(area);
                let icon_end = draw_file_icon(frame, p, (file_x, cy), &name, clip);
                let color = git_color(p, file.change);
                let end = ui_text(frame, app, icon_end + 6.0, row, &name, color, clip);
                let dir = file
                    .path
                    .parent()
                    .and_then(|d| root.and_then(|r| d.strip_prefix(r).ok()))
                    .map(|d| d.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let dir = match (&file.from, file.change) {
                    (Some(from), Change::Renamed) => {
                        format!("← {}", winc_editor::display_name(from))
                    }
                    _ if !flat => String::new(),
                    _ => dir,
                };
                let room = (clip.right() - end - 12.0).max(0.0);
                let full = frame.measure(FontKind::Ui, app.fonts.ui_small, &dir);
                if full > room || file.from.is_some() {
                    long_files.push(i);
                }
                if !dir.is_empty() {
                    let dir = truncate_end(frame, &dir, app.fonts.ui_small, room);
                    ui_text_small(frame, app, end + 8.0, row, &dir, p.text_dim, clip);
                }
                if hovered {
                    // 悬停在文件行上时，右端浮现「打开文件」「与工作区比较」「查看文件历史」三个快捷操作按钮。
                    let btn_s = 22.0;
                    let gap = 2.0;
                    let btn_y = (row.y + (row.h - btn_s) / 2.0).round();
                    let start_x = row.right() - 6.0 - 3.0 * btn_s - 2.0 * gap;
                    let actions = [
                        LogTarget::FileOpen(i),
                        LogTarget::FileCompareLocal(i),
                        LogTarget::FileHistory(i),
                    ];
                    for (k, target) in actions.into_iter().enumerate() {
                        let btn =
                            Rect::new(start_x + k as f32 * (btn_s + gap), btn_y, btn_s, btn_s);
                        let on = app.pointer.hover == Hover::Log(target);
                        draw_button(frame, p, btn, area, on, false);
                        let ink = if on { p.text } else { p.text_muted };
                        let (cx, cy) = center(btn);
                        match target {
                            LogTarget::FileOpen(_) => {
                                let (w, h) = (10.0, 12.0);
                                icons::document(
                                    frame,
                                    ((cx - w / 2.0).round(), (cy - h / 2.0).round()),
                                    (w, h),
                                    ink,
                                    area,
                                );
                            }
                            LogTarget::FileCompareLocal(_) => {
                                icons::compare(frame, (cx, cy), ink, area);
                            }
                            _ => {
                                icons::history(frame, (cx, cy), ink, area);
                            }
                        }
                        hits.push((btn.intersect(area), target));
                    }
                } else {
                    draw_stat(app, frame, file.lines, (row.right() - stat_w, row), area);
                }
                hits.push((row.intersect(area), LogTarget::File(i)));
            }
        }
    }
    y
}

/// 绘制文件一行右边的增删统计：「+12 −3」，后面五格小方块按增删的比例上色。
///
/// 每格 6 逻辑像素见方，间距 1 逻辑像素，整组靠行右端对齐。
fn draw_stat(
    app: &App,
    frame: &mut Frame<'_>,
    lines: Option<(usize, usize)>,
    (x, row): (f32, Rect),
    clip: Rect,
) {
    let p = app.palette;
    let Some((added, removed)) = lines else {
        ui_text_small(
            frame,
            app,
            x + 30.0,
            row,
            t!("git-log-file-binary"),
            p.text_dim,
            clip,
        );
        return;
    };
    let text = format!("+{added} −{removed}");
    let w = frame.measure(FontKind::Ui, app.fonts.ui_small, &text);
    let blocks_x = row.right() - 5.0 * 7.0 - 6.0;
    let end = ui_text_small(
        frame,
        app,
        blocks_x - 8.0 - w,
        row,
        &format!("+{added}"),
        p.git_added,
        clip,
    );
    ui_text_small(
        frame,
        app,
        end + 4.0,
        row,
        &format!("−{removed}"),
        p.git_deleted,
        clip,
    );
    let total = added + removed;
    let green = if total == 0 {
        0
    } else {
        ((added as f32 / total as f32) * 5.0).round() as usize
    };
    let red = if total == 0 { 0 } else { 5 - green };
    let cy = row.y + row.h / 2.0;
    for k in 0..5 {
        let color = if k < green {
            p.git_added
        } else if k < green + red {
            p.git_deleted
        } else {
            p.separator
        };
        let block = Rect::new(blocks_x + k as f32 * 7.0, cy - 3.0, 6.0, 6.0);
        frame.rect_clipped(block, color, 1.5, clip);
    }
}

/// 详情底部的「查找文件」搜索框：与快捷键提示占同一条，打开时替代提示行。
///
/// 放在底部而不是文件列表上方，是因为详情的内容整体滚动，跟着滚的搜索框会移出视野。键入的
/// 文字按路径筛选选中提交改动的文件（见 [`App::log_file_rows_of`]）。
pub(super) fn draw_file_filter(
    app: &mut App,
    frame: &mut Frame<'_>,
    parts: &LogParts,
    hits: &mut Vec<(Rect, LogTarget)>,
) {
    let p = app.palette;
    let area = parts.details;
    let row = Rect::new(area.x, area.bottom() - 30.0, area.w, 30.0);
    frame.rect(Rect::new(area.x, row.y, area.w, 1.0), p.separator);
    let rect = Rect::new(row.x + 10.0, row.y + 3.0, (row.w - 20.0).max(0.0), 24.0);
    let focused = app.focus == Focus::LogFileFilter;
    field_box(app, frame, rect, p.editor_bg, focused, false);
    icons::magnifier(
        frame,
        (rect.x + 14.0, rect.y + rect.h / 2.0),
        p.text_dim,
        rect,
    );
    let inside = Rect::new(rect.x + 26.0, rect.y, rect.w - 32.0, rect.h);
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
