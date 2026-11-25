//! Git 日志标签的绘制（1:1 还原 VS Code 插件 `DonJayamanne/gitHistoryVSCode`）。
//!
//! 布局与视觉结构完整对齐 `gitHistoryVSCode`：
//! - 顶部标题栏（`Header`）：分支面板开关、搜索输入框、`Search`、`Branch ▾`、`Author ▾`、
//!   `Clear`、`Refresh`，以及右侧辅助按钮（日期/路径筛选、提交数、定位 HEAD、上下/左右分栏切换）。
//! - 中部可拖拽分栏（`SplitPane`）：默认上下分栏（上方提交列表、下方提交详情），支持切换左右分栏
//!   （`gitHistory.sideBySide`）；点击详情右上角 `×` 折叠详情后列表占满 100%。
//! - 提交列表（`LogEntry`）：左侧平滑三次贝塞尔彩色提交图（白描边暗心节点），中部双行卡片
//!   （第一行提交说明支持 `:gitmoji:`，第二行 15×15 头像 + 按作者筛选眼睛图标 + 作者名 + 时间），
//!   右侧上下两行（上行红/绿/灰可点击引用徽章带 `×`，下行底色操作条 `<short7> ⧉ Soft Hard +Tag +Branch More`）。

pub(in crate::view::git) mod badges;
mod branches;
mod details;
mod files;
mod gitmoji;
pub(in crate::view::git) mod graph;

use badges::*;
use branches::*;
use details::*;
use files::*;
use gitmoji::gitmojify;
use graph::*;

use std::f32::consts::{FRAC_PI_2, PI};
use std::ops::Range;
use std::time::{SystemTime, UNIX_EPOCH};

use winc_core::graph::GraphRow;
use winc_editor::git::log::LogView;
use winc_editor::git::{Change, Commit, Ref};
use winc_i18n::t;
use winc_render::Stroke;

use crate::app::Focus;
use crate::app::git::log::{
    LOG_FILTER_GAP, LOG_LIST_PAD, LOG_ROW_HEIGHT, LogParts, LogPill, LogTarget, TextRun,
    details_top, display_time, is_relative_time, short, since_label,
};
use crate::app::{StatusEntry, StatusRow};
use crate::icons::Direction;
use crate::keymap::{self, Action};
use crate::layout::editor_content_height;
use crate::view::workspace::sidebar::git_color;
use crate::view::*;
use winc_editor::git::LogQuery;

/// 提交图中相邻两条车道的间距（逻辑像素，对齐 `svgGenerator.ts` `xOffset = 12`）。
pub(in crate::view::git) const LANE: f32 = 12.0;
/// 侧栏提交图线条粗细（供 `view::git::commit::graph` 复用）。
pub(in crate::view::git) const GRAPH_LINE: f32 = 1.8;
/// 换车道的线在拐弯处的圆角半径（逻辑像素，供侧栏提交图复用）。
const CORNER: f32 = 6.0;
/// 拐弯处圆弧的折线近似段数（段）。
const CORNER_STEPS: usize = 10;
/// 选中提交时，不在选中提交所在线上的线和圆点保留的不透明度（0 到 1）。
pub(in crate::view::git) const GRAPH_FADED: f32 = 0.55;
/// 提交图最多绘制的车道数（条）。
pub(in crate::view::git) const MAX_LANES: usize = 10;
/// 单个引用徽章的最大宽度（逻辑像素）。
const BADGE_MAX: f32 = 140.0;
/// 截短后徽章的最小宽度（逻辑像素）。
pub(in crate::view::git) const BADGE_MIN: f32 = 64.0;
/// 相邻两个徽章之间的间隔（逻辑像素）。
pub(in crate::view::git) const BADGE_GAP: f32 = 6.0;
/// 侧栏提交图各颜色序号对应的调色板下标。
const LANE_ORDER: [usize; 10] = [6, 3, 0, 7, 4, 1, 8, 5, 2, 9];
/// 详情一栏的左右留白（逻辑像素）。
const PAD: f32 = 18.0;
/// 详情中文件列表每行的高度（逻辑像素）。
const FILE_ROW: f32 = crate::app::git::log::LOG_FILE_ROW;

/// 绘制第 `index` 个文档（日志标签）。
///
/// # Panics
///
/// `index` 超出 `app.editor.documents` 的范围时 panic。
pub(in crate::view) fn draw_log(app: &mut App, frame: &mut Frame<'_>, index: usize) {
    let id = app.editor.documents[index].id;
    let parts = app.log_parts();
    let row_h = LOG_ROW_HEIGHT;
    let (scroll, rows) = {
        let doc = &app.editor.documents[index];
        (doc.scroll_line, doc.row_count())
    };
    let first = scroll.floor().max(0.0) as usize;
    let visible = (parts.list.h / row_h).ceil() as usize + 1;
    // 可见范围接近已读取的最后一个提交时，继续读取下一批。
    app.log_near_end(id, first + visible);

    let mut hits = Vec::new();
    let mut clipped = None;
    let mut long_files = Vec::new();
    let mut long_branches = Vec::new();
    let mut runs = Vec::new();
    let details_bottom;
    let branches_max;
    let shown = app.editor.documents[index]
        .log
        .as_deref()
        .and_then(|view| shown_oid(app, view));
    app.note_log_details_shown(shown);
    {
        let p = app.palette;
        frame.rect(app.layout.editor, p.editor_bg);
        frame.rect(parts.header, p.sidebar_bg);
    }
    // 筛选输入框需要更新自身的排版状态（供点击、拖选使用），先单独绘制。
    app.sync_log_filter();
    let search = draw_search(app, frame, &parts, &mut hits);
    hits.push((search, LogTarget::Search));
    // 分支面板顶部的搜索框同样要更新排版状态。
    if let Some(rect) = branches::branch_field(&parts) {
        branches::draw_branch_field(app, frame, rect, &mut hits);
    }
    let pills = match app.editor.documents[index].log.as_deref() {
        Some(view) => log_pills(app, frame, &view.query),
        None => Vec::new(),
    };
    let animate = app.animate_status();
    let pills = app.git.log.pills.sync(&LogPill::ALL, pills, animate);
    let has_details_content = app.git.log.details_open
        && parts.details.h >= 36.0
        && parts.details.w >= 120.0
        && app.editor.documents[index].log.as_deref().is_some_and(|v| {
            !v.commits.is_empty() && (v.selected.is_some() || v.compare.is_some())
        });
    {
        let app = &*app;
        let Some(view) = app.editor.documents[index].log.as_deref() else {
            return;
        };
        draw_header(app, frame, view, &parts, (search, &pills), &mut hits);
        draw_branches_button(app, frame, &parts, &mut hits);
        branches_max = draw_branches(app, frame, view, &parts, (&mut hits, &mut long_branches));
        draw_list(app, frame, view, &parts, scroll, &mut hits, &mut clipped);
        if app.git.log.details_open && parts.details.h >= 1.0 && parts.details.w >= 1.0 {
            draw_details_footer(app, frame, view, &parts);
            let opacity = app.git.log.fade.map_or(1.0, |c| 0.4 + 0.6 * c.progress());
            details_bottom = frame.with_opacity(opacity, |frame| {
                draw_details(
                    app,
                    frame,
                    view,
                    &parts,
                    (&mut hits, &mut long_files, &mut runs),
                )
            });
            // 在详情内容之后绘制顶右固定工具条（`Tree/List` + `× Close`），确保滚动时始终可见。
            if has_details_content {
                draw_details_top_actions(app, frame, view, &parts, &mut hits);
            }
        } else {
            details_bottom = parts.details.y;
        }
        // 分隔线压在列表与详情交界处，放在命中表最前。
        draw_split_resizer(app, frame, view, &parts, &mut hits);
    }
    // 详情右上角始终显示「Find file」输入框（1:1 对齐 `gitHistoryVSCode` `Commit/index.tsx:117-127`）。
    if has_details_content {
        draw_inline_file_filter(app, frame, &parts, &mut hits);
    }
    // 详情一栏的最大滚动位置。
    let details = parts.details;
    let footer_h = if details.h >= 60.0 { 30.0 } else { 0.0 };
    if let Some(view) = app.editor.documents[index].log.as_deref_mut() {
        let content = details_bottom + view.details_scroll - details.y + 16.0;
        let max = (content - (details.h - footer_h)).max(0.0);
        app.git.log.details_max = max;
        view.details_scroll = view.details_scroll.min(max);
    }
    app.git.log.branches_max = branches_max;
    app.git.log.branches_scroll = app.git.log.branches_scroll.min(branches_max);
    app.git.log.long_branches = long_branches;
    let panel = parts.branches;
    if panel.w > 0.0 {
        let track = Rect::new(panel.x, panel.y, panel.w - 1.0, panel.h);
        let scroll = app.git.log.branches_scroll;
        let content = branches_max + panel.h;
        draw_scrollbar(
            app,
            frame,
            ScrollArea::LogBranches,
            0,
            track,
            scroll,
            content,
        );
    }
    app.hits.log = hits;
    app.git.log.clipped = clipped;
    app.git.log.long_files = long_files;
    app.git.log.runs = runs;

    // 列表的滚动条紧贴列表右边缘。
    let top = {
        let view = app.editor.documents[index].log.as_deref();
        let extra = view.is_some_and(|v| app.log_uncommitted(v) > 0);
        LOG_LIST_PAD + if extra { row_h } else { 0.0 }
    };
    let content = editor_content_height(rows, parts.list.h, row_h) + top;
    draw_scrollbar(
        app,
        frame,
        ScrollArea::Editor,
        id.0,
        parts.list,
        (scroll * f64::from(row_h)) as f32,
        content,
    );
}

/// 绘制标题栏左侧的筛选输入框（对齐 `gitHistoryVSCode` `.textInput`）。
fn draw_search(
    app: &mut App,
    frame: &mut Frame<'_>,
    parts: &LogParts,
    hits: &mut Vec<(Rect, LogTarget)>,
) -> Rect {
    let p = app.palette;
    let search = parts.search();
    let focused = app.focus == Focus::LogFilter;
    let has_text = !app.git.log.filter.text().is_empty();
    field_box(app, frame, search, p.editor_bg, focused, false);
    icons::magnifier(
        frame,
        (search.x + 13.0, search.y + search.h / 2.0),
        p.text_dim,
        search,
    );
    let clear_reserve = if has_text { 22.0 } else { 6.0 };
    let area = Rect::new(
        search.x + 25.0,
        search.y,
        (search.w - 25.0 - clear_reserve).max(0.0),
        search.h,
    );
    let placeholder = match keymap::shortcut(Action::Find) {
        Some(key) => t!("git-log-filter-placeholder-key", key = key),
        None => t!("git-log-filter-placeholder").to_owned(),
    };
    let mut field = std::mem::take(&mut app.git.log.filter);
    let caret = draw_bar_field(app, frame, area, &mut field, &placeholder, focused);
    app.git.log.filter = field;
    if focused {
        app.ime.cursor = caret;
    }
    if has_text {
        let btn = Rect::new(
            search.right() - 20.0,
            search.y + (search.h - 18.0) / 2.0,
            18.0,
            18.0,
        );
        let hovered = app.pointer.hover == Hover::Log(LogTarget::ClearSearch);
        draw_button(frame, p, btn, search, hovered, false);
        let ink = if hovered { p.text } else { p.text_muted };
        icons::close(frame, center(btn), 7.0, ink, search);
        hits.push((btn, LogTarget::ClearSearch));
    }
    search
}

/// 绘制 `gitHistoryVSCode` 风格的标题栏按钮（扁平直角边框，主按钮为蓝色填充）。
fn draw_header_btn(
    app: &App,
    frame: &mut Frame<'_>,
    rect: Rect,
    label: &str,
    (primary, chevron, hovered): (bool, bool, bool),
    clip: Rect,
) {
    let p = app.palette;
    let (fill, ink, border) = if primary {
        let bg = if hovered {
            p.accent_fill.alpha(0.88)
        } else {
            p.accent_fill
        };
        (bg, p.text_on_accent, p.accent_fill)
    } else {
        let bg = if hovered {
            p.selected_inactive
        } else {
            p.hover
        };
        let bd = if hovered { p.accent } else { p.separator };
        (bg, if hovered { p.text } else { p.text_muted }, bd)
    };
    frame.rect_clipped(rect, fill, 2.0, clip);
    let outline =
        icons::rounded_outline(rect.x + 0.5, rect.y + 0.5, rect.w - 1.0, rect.h - 1.0, 2.0);
    frame.stroke(&outline, 1.0, border, clip);
    let text_clip = if chevron {
        Rect::new(rect.x, rect.y, (rect.w - 16.0).max(0.0), rect.h).intersect(clip)
    } else {
        rect.intersect(clip)
    };
    ui_text_small(frame, app, rect.x + 9.0, rect, label, ink, text_clip);
    if chevron {
        let pos = (rect.right() - 9.0, rect.y + rect.h / 2.0);
        icons::chevron(frame, pos, 5.5, Direction::Down, ink, clip);
    }
}

/// 绘制标题栏（1:1 对齐 `gitHistoryVSCode` `components/Header/index.tsx`）：
/// `[Branches] [Search Input] [Search] [Branch ▾] [Author ▾] [Clear] [Refresh]  ...  [Date] [Path] [N commits] [◎] [Split]`。
fn draw_header(
    app: &App,
    frame: &mut Frame<'_>,
    view: &LogView,
    parts: &LogParts,
    (search, _pills): (Rect, &[StatusRow<LogPill>]),
    hits: &mut Vec<(Rect, LogTarget)>,
) {
    let p = app.palette;
    let header = parts.header;
    frame.rect(
        Rect::new(header.x, header.bottom() - 1.0, header.w, 1.0),
        p.separator,
    );
    let query = &view.query;
    let btn_h = search.h;
    let y = search.y;
    let size = app.fonts.ui_small;
    let gap = LOG_FILTER_GAP;

    // 先算右端工具栏（分栏切换 + 定位 HEAD + 提交数），留出左侧控件空间。
    let icon_s = 26.0;
    let layout_btn = Rect::new(
        header.right() - 10.0 - icon_s,
        header.y + (header.h - icon_s) / 2.0,
        icon_s,
        icon_s,
    );
    let hovered = app.pointer.hover == Hover::Log(LogTarget::LayoutToggle);
    draw_button(frame, p, layout_btn, header, hovered, parts.side_by_side);
    let ink = if hovered || parts.side_by_side {
        p.accent
    } else {
        p.text_muted
    };
    if parts.side_by_side {
        icons::split(frame, center(layout_btn), ink, layout_btn);
    } else {
        icons::panel(frame, center(layout_btn), false, ink, layout_btn);
    }
    hits.push((layout_btn, LogTarget::LayoutToggle));

    let jump_head = Rect::new(layout_btn.x - 4.0 - icon_s, layout_btn.y, icon_s, icon_s);
    let hovered = app.pointer.hover == Hover::Log(LogTarget::JumpHead);
    draw_button(frame, p, jump_head, header, hovered, false);
    icons::target_head(
        frame,
        center(jump_head),
        if hovered { p.text } else { p.text_muted },
        jump_head,
    );
    hits.push((jump_head, LogTarget::JumpHead));

    let count = view.commits.len();
    let more = if view.complete { "" } else { "+" };
    let quiet = app.log_list_quiet();
    let note = if view.stale && !quiet {
        t!("git-log-refreshing").to_owned()
    } else if view.is_loading() && count == 0 {
        if quiet {
            String::new()
        } else {
            t!("git-log-loading").to_owned()
        }
    } else {
        t!("git-log-commit-count", count = count, more = more)
    };
    let note_w = frame.measure(FontKind::Ui, size, &note);
    let note_x = jump_head.x - 10.0 - note_w;
    ui_text_small(frame, app, note_x, header, &note, p.text_dim, header);
    let right_limit = note_x - 8.0;

    let mut x = search.right() + gap;

    // 1. `Search` 按钮（加载中显示 `Loading...`）。
    let search_lbl = if view.is_loading() && !quiet {
        t!("git-log-loading")
    } else {
        t!("git-log-search-button")
    };
    let w = frame.measure(FontKind::Ui, size, search_lbl) + 18.0;
    if x + w <= right_limit {
        let rect = Rect::new(x, y, w, btn_h);
        let hovered = app.pointer.hover == Hover::Log(LogTarget::SearchButton);
        draw_header_btn(
            app,
            frame,
            rect,
            search_lbl,
            (false, false, hovered),
            header,
        );
        hits.push((rect, LogTarget::SearchButton));
        x = rect.right() + gap;
    }

    // 2. `Branch ▾` 主色下拉按钮（1:1 对齐 `Header/branch/index.tsx` `bsStyle="primary"`）。
    let branch_text = match query.branches.as_slice() {
        [] => t!("git-log-all-branches").to_owned(),
        [one] => one.clone(),
        many => t!("git-log-filter-branches", count = many.len()),
    };
    let bw = frame.measure(FontKind::Ui, size, &branch_text).min(150.0) + 28.0;
    if x + bw <= right_limit {
        let rect = Rect::new(x, y, bw, btn_h);
        let hovered =
            app.pointer.hover == Hover::Log(LogTarget::Branch) || app.picker_filters_log();
        draw_header_btn(
            app,
            frame,
            rect,
            &branch_text,
            (true, true, hovered),
            header,
        );
        hits.push((rect, LogTarget::Branch));
        x = rect.right() + gap;
    }

    // 3. `Author ▾` 下拉按钮（行历史模式下隐藏，1:1 对齐 `Header/author/index.tsx:74`）。
    if query.lines.is_none() {
        let author_text = match query.authors.as_slice() {
            [] => t!("menu-log-all-authors").to_owned(),
            [me] if !app.git.my_email.is_empty() && *me == app.git.my_email => {
                t!("menu-log-author-me").to_owned()
            }
            [one] => one.clone(),
            many => t!("git-log-filter-authors", count = many.len()),
        };
        let author_on = !query.authors.is_empty();
        let aw = frame.measure(FontKind::Ui, size, &author_text).min(130.0) + 28.0;
        if x + aw <= right_limit {
            let rect = Rect::new(x, y, aw, btn_h);
            let hovered = app.pointer.hover == Hover::Log(LogTarget::Author);
            draw_header_btn(
                app,
                frame,
                rect,
                &author_text,
                (author_on || true, true, hovered),
                header,
            );
            hits.push((rect, LogTarget::Author));
            x = rect.right() + gap;
        }
    }

    // 4. `Clear` 按钮（对齐 `Header/index.tsx:133`）。
    let clear_lbl = t!("git-log-clear-button");
    let cw = frame.measure(FontKind::Ui, size, clear_lbl) + 18.0;
    if x + cw <= right_limit {
        let rect = Rect::new(x, y, cw, btn_h);
        let hovered = app.pointer.hover == Hover::Log(LogTarget::ClearFilters);
        draw_header_btn(app, frame, rect, clear_lbl, (false, false, hovered), header);
        hits.push((rect, LogTarget::ClearFilters));
        x = rect.right() + gap;
    }

    // 5. `Refresh` 按钮（对齐 `Header/index.tsx:136`）。
    let refresh_lbl = t!("git-log-refresh-button");
    let rw = frame.measure(FontKind::Ui, size, refresh_lbl) + 18.0;
    if x + rw <= right_limit {
        let rect = Rect::new(x, y, rw, btn_h);
        let hovered = app.pointer.hover == Hover::Log(LogTarget::Refresh);
        draw_header_btn(
            app,
            frame,
            rect,
            refresh_lbl,
            (false, false, hovered),
            header,
        );
        hits.push((rect, LogTarget::Refresh));
        x = rect.right() + gap;
    }

    // 6. 额外实用筛选（日期、路径，有空间时展示；文件历史时展示文件名胶囊）。
    if let Some(path) = &query.path {
        let shown = app
            .git
            .root
            .as_deref()
            .and_then(|r| path.strip_prefix(r).ok())
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        let text = t!("git-log-filter-file", path = shown);
        let fw = frame.measure(FontKind::Ui, size, &text).min(220.0) + 18.0;
        if x + fw <= right_limit {
            let rect = Rect::new(x, y, fw, btn_h);
            frame.rounded(rect, p.hover, 2.0);
            ui_text_small(frame, app, rect.x + 9.0, rect, &text, p.text_muted, rect);
        }
    } else {
        let date_on = query.since_days.is_some() || query.since.is_some();
        let date_lbl = match (query.since_days, query.since.as_deref()) {
            (Some(days), _) => since_label(days),
            (None, Some(since)) => since.to_owned(),
            (None, None) => t!("git-log-filter-date").to_owned(),
        };
        let dw = frame.measure(FontKind::Ui, size, &date_lbl).min(110.0) + 26.0;
        if x + dw <= right_limit {
            let rect = Rect::new(x, y, dw, btn_h);
            let hovered = app.pointer.hover == Hover::Log(LogTarget::Date);
            draw_header_btn(
                app,
                frame,
                rect,
                &date_lbl,
                (date_on, true, hovered),
                header,
            );
            hits.push((rect, LogTarget::Date));
            x = rect.right() + gap;
        }

        let path_on = !query.paths.is_empty();
        let path_lbl = match query.paths.as_slice() {
            [] => t!("git-log-filter-path").to_owned(),
            [one] => one.clone(),
            many => t!("git-log-filter-paths", count = many.len()),
        };
        let pw = frame.measure(FontKind::Ui, size, &path_lbl).min(110.0) + 26.0;
        if x + pw <= right_limit {
            let rect = Rect::new(x, y, pw, btn_h);
            let hovered = app.pointer.hover == Hover::Log(LogTarget::Path);
            draw_header_btn(
                app,
                frame,
                rect,
                &path_lbl,
                (path_on, true, hovered),
                header,
            );
            hits.push((rect, LogTarget::Path));
        }
    }
}

/// 筛选下拉 `pill` 的目标、名字，以及是否在按它筛选。
fn pill_info(pill: LogPill, query: &LogQuery) -> (LogTarget, &'static str, bool) {
    let (name, on) = match pill {
        LogPill::Branch => (t!("git-log-filter-branch"), !query.branches.is_empty()),
        LogPill::Author => (t!("git-log-filter-author"), !query.authors.is_empty()),
        LogPill::Date => (
            t!("git-log-filter-date"),
            query.since_days.is_some() || query.since.is_some(),
        ),
        LogPill::Path => (t!("git-log-filter-path"), !query.paths.is_empty()),
    };
    (pill.target(), name, on)
}

/// 本帧标题栏的各个筛选下拉过渡状态。
fn log_pills(app: &App, frame: &mut Frame<'_>, query: &LogQuery) -> Vec<StatusEntry<LogPill>> {
    let all = t!("git-log-filter-all");
    let size = app.fonts.ui_small;
    let value = |pill| match pill {
        LogPill::Branch => match query.branches.as_slice() {
            [] => all.to_owned(),
            [one] => one.clone(),
            many => t!("git-log-filter-branches", count = many.len()),
        },
        LogPill::Author => match query.authors.as_slice() {
            [] => all.to_owned(),
            [me] if !app.git.my_email.is_empty() && *me == app.git.my_email => {
                t!("menu-log-author-me").to_owned()
            }
            [one] => one.clone(),
            many => t!("git-log-filter-authors", count = many.len()),
        },
        LogPill::Date => match (query.since_days, query.since.as_deref()) {
            (Some(days), _) => since_label(days),
            (None, Some(since)) => since.to_owned(),
            (None, None) => all.to_owned(),
        },
        LogPill::Path => match query.paths.as_slice() {
            [] => all.to_owned(),
            [one] => one.clone(),
            many => t!("git-log-filter-paths", count = many.len()),
        },
    };
    let pills = LogPill::ALL.into_iter();
    let pills = pills.filter(|p| *p != LogPill::Path || query.path.is_none());
    pills
        .map(|pill| {
            let text = value(pill);
            let (_, name, _) = pill_info(pill, query);
            let name_w = frame.measure(FontKind::Ui, size, name);
            let value_w = frame.measure(FontKind::Ui, size, &text).min(160.0);
            let width = 10.0 + name_w + 6.0 + value_w + 20.0;
            StatusEntry {
                key: pill,
                text,
                item: None,
                content: width,
                width,
            }
        })
        .collect()
}

/// 返回 `HEAD` 提交在提交图中的车道和颜色；该提交尚未读取时返回 `None`。
fn head_lane(view: &LogView) -> Option<(usize, u8)> {
    let graph = view.graph.get(view.head_row()?)?;
    Some((graph.lane, graph.color))
}

/// 绘制列表最上方的「未提交的更改（N）」双行卡片。
fn draw_uncommitted(
    app: &App,
    frame: &mut Frame<'_>,
    (list, y, text_x): (Rect, f32, f32),
    (lane, head_first): (Option<(f32, usize, u8)>, bool),
    count: usize,
    hits: &mut Vec<(Rect, LogTarget)>,
) {
    let p = app.palette;
    let lh = LOG_ROW_HEIGHT;
    let row = Rect::new(list.x, y, list.w, lh);
    if row.bottom() <= list.y {
        return;
    }
    let hovered = app.pointer.hover == Hover::Log(LogTarget::Uncommitted);
    if hovered {
        frame.rect_clipped(row, p.hover, 0.0, list);
    }
    frame.rect_clipped(
        Rect::new(list.x, row.bottom() - 1.0, list.w, 1.0),
        p.separator,
        0.0,
        list,
    );
    if let Some((x0, lane, color)) = lane {
        let ink = lane_color(p, color);
        let cx = x0 + lane.min(MAX_LANES - 1) as f32 * LANE + LANE / 2.0;
        let cy = y + lh / 2.0;
        if head_first {
            let dash_color = ink.alpha(ink.a * GRAPH_FADED);
            let mut dy = cy + 6.0;
            let end_y = y + lh;
            while dy < end_y {
                let seg = (end_y - dy).min(4.0);
                frame.rect_clipped(Rect::new(cx - 1.2, dy, 2.4, seg), dash_color, 0.5, list);
                dy += 7.0;
            }
        }
        let r = 4.6;
        frame.rect_clipped(Rect::new(cx - r, cy - r, 2.0 * r, 2.0 * r), ink, r, list);
        let inner = r - 1.8;
        let bg = if hovered {
            crate::view::workspace::panel::over(p.editor_bg, p.hover)
        } else {
            p.editor_bg
        };
        frame.rect_clipped(
            Rect::new(cx - inner, cy - inner, 2.0 * inner, 2.0 * inner),
            bg,
            inner,
            list,
        );
    }
    let line1 = Rect::new(text_x, y + 7.0, list.right() - text_x - 16.0, 22.0);
    let line2 = Rect::new(text_x, y + 30.0, list.right() - text_x - 16.0, 18.0);
    ui_text(
        frame,
        app,
        text_x,
        line1,
        t!("git-log-uncommitted"),
        p.text_muted,
        list,
    );
    ui_text_small(
        frame,
        app,
        text_x,
        line2,
        &t!("git-log-uncommitted-files", count = count),
        p.text_dim,
        list,
    );
    hits.push((row.intersect(list), LogTarget::Uncommitted));
}

/// 在提交行右下角绘制 `gitHistoryVSCode` 风格的快捷操作条（`.media-image > div.buttons > div`）：
/// `[a1b2c3d ⧉  Soft  Hard  +Tag  +Branch  More]`。
///
/// 返回操作条的最左 x 坐标。
fn draw_entry_buttons(
    app: &App,
    frame: &mut Frame<'_>,
    commit: &Commit,
    row_idx: usize,
    (right_x, y, avail_w): (f32, f32, f32),
    clip: Rect,
    hits: &mut Vec<(Rect, LogTarget)>,
) -> f32 {
    let p = app.palette;
    let size = app.fonts.ui_small * 0.92;
    let strip_h = 20.0;
    let short_oid = short(&commit.oid);

    // 空间充足时完整绘制 6 个按钮（1:1 `LogEntry/index.tsx:127-207`），窄屏退化为 Hash + More。
    let compact = avail_w < 330.0;
    let actions: Vec<(LogTarget, &str)> = if compact {
        vec![(LogTarget::RowMore(row_idx), t!("git-log-row-more"))]
    } else {
        vec![
            (LogTarget::RowResetSoft(row_idx), t!("git-log-row-soft")),
            (LogTarget::RowResetHard(row_idx), t!("git-log-row-hard")),
            (LogTarget::RowNewTag(row_idx), t!("git-log-row-tag")),
            (LogTarget::RowNewBranch(row_idx), t!("git-log-row-branch")),
            (LogTarget::RowMore(row_idx), t!("git-log-row-more")),
        ]
    };

    let hash_w = frame.measure(FontKind::Mono, size, short_oid) + 22.0;
    let item_gap = 9.0;
    let mut actions_w = 0.0;
    let mut widths = Vec::with_capacity(actions.len());
    for (_, label) in &actions {
        let w = frame.measure(FontKind::Ui, size, label);
        widths.push(w);
        actions_w += item_gap + w;
    }
    let total_w = 8.0 + hash_w + actions_w + 8.0;
    let x0 = right_x - total_w;
    let strip = Rect::new(x0, y, total_w, strip_h);
    // 对齐 `main.css:148`: `background-color: var(--vscode-scrollbarSlider-background)`。
    frame.rect_clipped(strip, p.hover, 3.0, clip);

    // 1. `<short7> ⧉`（复制提交号，等宽字体）。
    let hash_rect = Rect::new(x0 + 6.0, y, hash_w, strip_h);
    let hash_hovered = app.pointer.hover == Hover::Log(LogTarget::RowCopyHash(row_idx));
    let hash_ink = if hash_hovered { p.text } else { p.text_muted };
    let (ma, md) = (app.metrics.mono.ascent, app.metrics.mono.descent);
    let mscale = size / app.fonts.code;
    let mbase = baseline_in(hash_rect, ma * mscale, md * mscale);
    let hash_end = frame.text(
        FontKind::Mono,
        size,
        hash_rect.x + 2.0,
        mbase,
        short_oid,
        hash_ink,
        clip,
    );
    icons::copy(frame, (hash_end + 10.0, y + strip_h / 2.0), hash_ink, clip);
    hits.push((hash_rect.intersect(clip), LogTarget::RowCopyHash(row_idx)));

    // 2. `Soft` / `Hard` / `+Tag` / `+Branch` / `More`（VS Code link 蓝色）。
    let (ua, ud) = (app.metrics.ui.ascent, app.metrics.ui.descent);
    let uscale = size / app.fonts.ui;
    let ubase = baseline_in(strip, ua * uscale, ud * uscale);
    let mut cx = hash_rect.right() + item_gap;
    for ((target, label), w) in actions.into_iter().zip(widths) {
        let btn = Rect::new(cx - 3.0, y, w + 6.0, strip_h);
        let hovered = app.pointer.hover == Hover::Log(target);
        if hovered {
            frame.rect_clipped(btn, p.selected_inactive, 2.0, clip);
        }
        let ink = if hovered { p.text } else { p.accent };
        frame.text(FontKind::Ui, size, cx, ubase, label, ink, clip);
        hits.push((btn.intersect(clip), target));
        cx += w + item_gap;
    }
    x0
}

fn draw_list(
    app: &App,
    frame: &mut Frame<'_>,
    view: &LogView,
    parts: &LogParts,
    scroll: f64,
    hits: &mut Vec<(Rect, LogTarget)>,
    clipped: &mut Option<usize>,
) {
    let p = app.palette;
    let list = parts.list;
    let row_h = LOG_ROW_HEIGHT;
    if view.commits.is_empty() {
        let (title, detail) = match &view.error {
            Some(e) => (t!("git-log-error"), Some(e.as_str())),
            None if view.is_loading() && app.log_list_quiet() => return,
            None if view.is_loading() => (t!("git-log-loading-commits"), None),
            None if view.query.filtered() => (
                t!("git-log-no-matches"),
                Some(t!("git-log-no-matches-detail")),
            ),
            None => (t!("git-log-empty"), Some(t!("git-log-empty-detail"))),
        };
        let whole = if parts.side_by_side {
            Rect::new(list.x, list.y, parts.details.right() - list.x, list.h)
        } else {
            Rect::new(list.x, list.y, list.w, parts.details.bottom() - list.y)
        };
        draw_empty_state(app, frame, (whole, whole), title, detail);
        return;
    }

    let first = scroll.floor().max(0.0) as usize;
    let visible = (list.h / row_h).ceil() as usize + 1;
    let rows = first.saturating_sub(1)..(first + visible).min(view.commits.len());
    let around = first.saturating_sub(visible)..first + 2 * visible;
    let lanes = view.graph_width(around).min(MAX_LANES);
    let graph_x = list.x + 10.0;
    // 对齐 `BranchGraph/index.tsx:29-35`：按搜索词、作者或文件筛选时隐藏提交图。
    let graphless = view.query.graphless();
    let text_x = if graphless {
        list.x + 14.0
    } else {
        graph_x + lanes as f32 * LANE + 12.0
    };
    let right_edge = list.right() - SCROLLBAR_HIT - 8.0;

    // 高亮选中提交所在的线（比较两个版本时不高亮）。
    let line = match app.pointer.hover {
        Hover::Log(LogTarget::Line(_, l)) => Some(l),
        _ => view
            .selected
            .filter(|_| view.compare.is_none())
            .and_then(|i| view.graph.get(i))
            .map(|g| g.line),
    };

    // 工作区有改动时，第一个提交上方增加「未提交的更改」卡片。
    let uncommitted = app.log_uncommitted(view);
    let top = LOG_LIST_PAD + if uncommitted > 0 { row_h } else { 0.0 };
    if uncommitted > 0 {
        let y = list.y + LOG_LIST_PAD - (scroll * f64::from(row_h)) as f32;
        let lane = (!graphless)
            .then(|| head_lane(view).map(|(lane, color)| (graph_x, lane, color)))
            .flatten();
        let head_first = view.head_row() == Some(0);
        draw_uncommitted(
            app,
            frame,
            (list, y, text_x),
            (lane, head_first),
            uncommitted,
            hits,
        );
    }

    for i in rows {
        let commit = &view.commits[i];
        let y = list.y + top + (i as f64 - scroll) as f32 * row_h;
        let row = Rect::new(list.x, y, list.w, row_h);
        let is_active = match &view.compare {
            Some(c) => rev_matches(commit, &c.from) || rev_matches(commit, &c.to),
            None => view.selected == Some(i),
        };
        let is_compare_base = app.git.log.compare_selected.as_deref() == Some(commit.oid.as_str());
        let hovered = matches!(
            app.pointer.hover,
            Hover::Log(
                LogTarget::Row(r)
                    | LogTarget::Avatar(r)
                    | LogTarget::RowFilterAuthor(r)
                    | LogTarget::RowCopyHash(r)
                    | LogTarget::RowResetSoft(r)
                    | LogTarget::RowResetHard(r)
                    | LogTarget::RowNewTag(r)
                    | LogTarget::RowNewBranch(r)
                    | LogTarget::RowMore(r)
                    | LogTarget::RefCheckout(r, _)
                    | LogTarget::RefRemove(r, _)
            ) if r == i
        );
        let fill = if is_active {
            Some(p.accent.alpha(if p.dark { 0.11 } else { 0.09 }))
        } else if hovered {
            Some(p.hover)
        } else {
            None
        };
        if let Some(color) = fill {
            frame.rect_clipped(row, color, 0.0, list);
        }
        // 对齐 `main.css:104-108`：每条 `.log-entry` 底部画 1px 分隔线。
        frame.rect_clipped(
            Rect::new(list.x, row.bottom() - 1.0, list.w, 1.0),
            p.separator,
            0.0,
            list,
        );

        // 左侧提交图（居中于双行卡片）。
        if let Some(graph) = view.graph.get(i).filter(|_| !graphless) {
            let cuts = long_cuts(view, i, line);
            draw_graph_row(
                app,
                frame,
                (graph, line, &cuts),
                (commit, i),
                (graph_x, y, fill),
                list,
                hits,
            );
        }

        // 右侧双行 `.media-image`：
        // 上行：红/绿/灰引用徽章（Remote -> Head/Branch -> Tag，带 `×` / 可签出）；
        // 下行：快捷操作条 `[<short7> ⧉  Soft  Hard  +Tag  +Branch  More]`。
        let ref_max_w = ((right_edge - text_x) * 0.48).max(0.0);
        let refs_left = draw_githistory_refs(
            app,
            frame,
            commit,
            i,
            (right_edge, y + 6.0, ref_max_w),
            list,
            hits,
        );
        let buttons_left = draw_entry_buttons(
            app,
            frame,
            commit,
            i,
            (right_edge, y + 30.0, (right_edge - text_x).max(0.0)),
            list,
            hits,
        );

        // 中部 Line 1：`.commit-subject`（支持 `:gitmoji:`，选中时变蓝，合并提交灰显）。
        let line1_rect = Rect::new(text_x, y + 5.0, (refs_left - text_x - 10.0).max(0.0), 22.0);
        let subject_raw = gitmojify(&commit.subject);
        let merge = commit.parents.len() > 1;
        let subject_color = if is_active {
            p.accent
        } else if merge {
            p.text_muted
        } else {
            p.text
        };
        let full_w = frame.measure(FontKind::Ui, app.fonts.ui, &subject_raw);
        let subject_clip = line1_rect.intersect(list);
        let shown_subject = truncate_end(frame, &subject_raw, app.fonts.ui, line1_rect.w);
        if hovered && full_w > line1_rect.w {
            *clipped = Some(i);
        }
        // 高亮搜索关键词匹配项。
        for range in matches(&shown_subject, view.query.text.trim()) {
            let font = app.fonts.ui;
            let left = text_x + frame.measure(FontKind::Ui, font, &shown_subject[..range.start]);
            let w = frame.measure(FontKind::Ui, font, &shown_subject[range]);
            let mark = Rect::new(left - 1.0, line1_rect.y + 3.0, w + 2.0, line1_rect.h - 6.0);
            frame.rect_clipped(mark, p.search_match, 3.0, subject_clip);
        }
        ui_text(
            frame,
            app,
            text_x,
            line1_rect,
            &shown_subject,
            subject_color,
            subject_clip,
        );

        // 中部 Line 2：15×15 头像 + `[Eye]` 按作者筛选 + 作者名 + `on <时间>`（0.7 不透明度）。
        let line2_rect = Rect::new(
            text_x,
            y + 30.0,
            (buttons_left - text_x - 10.0).max(0.0),
            20.0,
        );
        let line2_clip = line2_rect.intersect(list);
        let mut ax = text_x;
        let cy2 = line2_rect.y + line2_rect.h / 2.0;
        draw_avatar(app, frame, commit, (ax + 7.5, cy2), line2_clip);
        let avatar_hit = Rect::new(ax, line2_rect.y, 16.0, line2_rect.h).intersect(line2_clip);
        hits.push((avatar_hit, LogTarget::Avatar(i)));
        ax += 20.0;

        // 眼睛按钮：点击按此作者筛选（对齐 `Commit/Author/index.tsx:22-31`）。
        let eye_rect = Rect::new(ax, line2_rect.y + 1.0, 18.0, 18.0);
        if eye_rect.right() <= line2_rect.right() {
            let eye_hovered = app.pointer.hover == Hover::Log(LogTarget::RowFilterAuthor(i));
            if eye_hovered {
                frame.rect_clipped(eye_rect, p.selected_inactive, 3.0, line2_clip);
            }
            let eye_ink = if eye_hovered {
                p.text
            } else if is_active {
                p.accent
            } else {
                p.text_muted
            };
            icons::eye(frame, center(eye_rect), 0.72, eye_ink, line2_clip);
            hits.push((
                eye_rect.intersect(line2_clip),
                LogTarget::RowFilterAuthor(i),
            ));
            ax = eye_rect.right() + 4.0;
        }

        let author_ink = if is_active { p.accent } else { p.text_muted };
        let date_ink = author_ink.alpha(author_ink.a * 0.7);
        let date_str = t!("git-log-on-date", date = commit.local.as_str());
        let name_end = ui_text_small(
            frame,
            app,
            ax,
            line2_rect,
            &commit.author,
            author_ink,
            line2_clip,
        );
        let time_end = ui_text_small(
            frame,
            app,
            name_end + 4.0,
            line2_rect,
            &date_str,
            date_ink,
            line2_clip,
        );
        if is_compare_base {
            let badge_txt = t!("git-log-compare-base-badge");
            let bw = frame.measure(FontKind::Ui, app.fonts.ui_small * 0.9, badge_txt) + 10.0;
            let bx = time_end + 8.0;
            if bx + bw <= line2_rect.right() {
                let brect = Rect::new(bx, line2_rect.y + 2.0, bw, 16.0);
                frame.rect_clipped(brect, p.accent.alpha(0.22), 3.0, line2_clip);
                ui_text_small(frame, app, bx + 5.0, brect, badge_txt, p.accent, line2_clip);
            }
        }

        // 整行点击命中放在子按钮之后，确保按钮优先命中。
        hits.push((row.intersect(list), LogTarget::Row(i)));
    }
}

/// 绘制列表与详情之间的可拖拽分隔条（对齐 `main.css:304-350` `.Resizer.horizontal` / `.Resizer.vertical`）。
fn draw_split_resizer(
    app: &App,
    frame: &mut Frame<'_>,
    view: &LogView,
    parts: &LogParts,
    hits: &mut Vec<(Rect, LogTarget)>,
) {
    if !app.git.log.details_open || view.commits.is_empty() {
        return;
    }
    let p = app.palette;
    let list = parts.list;
    let active = app.log_dividing() || app.pointer.hover == Hover::Log(LogTarget::Divider);
    let (hit_rect, visual_rect) = if parts.side_by_side {
        (
            Rect::new(list.right() - 3.0, list.y, 7.0, list.h),
            if active {
                Rect::new(list.right() - 1.5, list.y, 3.0, list.h)
            } else {
                Rect::new(list.right() - 1.0, list.y, 1.5, list.h)
            },
        )
    } else {
        (
            Rect::new(list.x, list.bottom() - 4.0, list.w, 8.0),
            if active {
                Rect::new(list.x, list.bottom() - 1.5, list.w, 3.0)
            } else {
                Rect::new(list.x, list.bottom() - 1.5, list.w, 2.5)
            },
        )
    };
    let color = if active { p.accent } else { p.separator };
    frame.rect(visual_rect, color);
    // 命中范围插入最前，优先于列表行与详情内容。
    hits.insert(0, (hit_rect, LogTarget::Divider));
}

/// 返回可点击的提交号的颜色：平时为强调色，悬停时为正文色。
fn link(p: &Palette, hovered: bool) -> Color {
    if hovered { p.text } else { p.accent }
}

/// 用等宽字体绘制一段文字（提交号），在 `row` 内垂直居中，返回末尾的 x 坐标（逻辑像素）。
pub(in crate::view) fn mono(
    app: &App,
    frame: &mut Frame<'_>,
    (x, row): (f32, Rect),
    text: &str,
    color: Color,
    clip: Rect,
) -> f32 {
    let (ascent, descent) = (app.metrics.mono.ascent, app.metrics.mono.descent);
    let size = app.fonts.ui_small;
    let scale = size / app.fonts.code;
    let baseline = baseline_in(row, ascent * scale, descent * scale);
    frame.text(FontKind::Mono, size, x, baseline, text, color, clip)
}

/// 把一段文字按宽度 `width`（逻辑像素）折行，返回各行的文字。
pub(in crate::view) fn wrap<'a>(
    frame: &mut Frame<'_>,
    size: f32,
    text: &'a str,
    width: f32,
) -> Vec<&'a str> {
    crate::layout::wrap_text(text, width, |s| frame.measure(FontKind::Ui, size, s))
}

#[cfg(test)]
mod tests;

/// 返回 `text` 中不区分大小写匹配 `needle` 的各段（字节范围，互不重叠）；`needle` 为空时返回空。
fn matches(text: &str, needle: &str) -> Vec<Range<usize>> {
    let same = |a: char, b: char| a == b || a.to_lowercase().eq(b.to_lowercase());
    let match_at = |start: usize| {
        let mut chars = text[start..].chars();
        let mut end = start;
        for n in needle.chars() {
            let c = chars.next().filter(|&c| same(c, n))?;
            end += c.len_utf8();
        }
        Some(end)
    };
    let mut found = Vec::new();
    let mut at = 0;
    while !needle.is_empty() && at < text.len() {
        match match_at(at) {
            Some(end) => {
                found.push(at..end);
                at = end;
            }
            None => at += text[at..].chars().next().map_or(1, char::len_utf8),
        }
    }
    found
}

/// 返回侧栏提交图第 `color` 种颜色的线色。
pub(in crate::view::git) fn lane_color(p: &Palette, color: u8) -> Color {
    let i = LANE_ORDER[color as usize % LANE_ORDER.len()];
    p.badges[i % p.badges.len()].0
}
