//! 提交视图底部的储藏：一组可以收起的行，每条储藏一行。
//!
//! 一组贴在改动列表下面，展开、收起时像抽屉一样往上拉开、往下收回：标题跟着上升，各行从下沿底下
//! 露出来，上面的改动列表随之变矮，节奏和文件树展开一个目录相同（`TREE_ANIMATION`）。
//!
//! 悬停时右端是「应用」和「⋯」（应用、应用并删除、删除），右键同样是这个菜单。点击一条储藏时在
//! 单独的预览标签里打开它的第一个改动文件，同时在这一行下面列出它改动的文件，点击其中一个即在
//! 预览标签里查看；差异标签里的上一个、下一个文件同样在这些文件之间切换。储藏时一起收进去的
//! 未跟踪文件排在后面。
//!
//! 一组最多露出 [`STASH_ROWS`] 行，再多时在组内上下滚动：滚轮、拖动右边细条，或者按住一行、
//! 按住行下面的空白直接拖（见 [`App::begin_stash_pan`]）。这样条数再多也不挤改动列表，不必先
//! 从菜单里挑选。

use winc_editor::DocumentId;
use winc_editor::git::{CommitFile, Stash};

use super::*;

/// 储藏一组里露出各行的范围：标题下面到这一组的下沿，下面留出与上沿相同的空隙。
pub(crate) fn stash_body(area: Rect) -> Rect {
    let top = area.y + STASH_GAP + COMMIT_ROW;
    Rect::new(
        area.x,
        top,
        area.w,
        (area.bottom() - top - STASH_GAP).max(0.0),
    )
}

/// 储藏一组展开时最多露出几行（行）：再多就把上面的改动列表挤得太矮，改为在组内滚动。
pub(crate) const STASH_ROWS: usize = 5;

/// 正在查看的储藏下面最多列出的文件行数（行），包括「还有 N 个文件」：再多时上方的改动列表
/// 高度过小。列出的一段总包含正在查看的文件，其余文件通过差异标签里的上一个、下一个文件切换。
const STASH_FILE_ROWS: usize = 6;

/// 储藏一组展开时标题下面的一行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StashLine {
    /// 第几条储藏（`GitState::stashes` 的下标）。
    Stash(usize),
    /// 正在看的储藏里的第几个文件（`StashView::files` 的下标）。
    File(usize),
    /// 正在看的储藏里没有列出的文件个数。
    MoreFiles(usize),
}

/// 储藏一组上下各留的空隙（逻辑像素）：列表间隔（`SIDEBAR_LIST_GAP`）的一半。这一组上下都是
/// 分隔线，分隔线本身已经隔开了相邻区域，留半个间隔即可。
pub(crate) const STASH_GAP: f32 = SIDEBAR_LIST_GAP / 2.0;
/// 储藏一组收着时的高度（逻辑像素）：标题一行，上下各留 [`STASH_GAP`]。
const STASH_CLOSED: f32 = COMMIT_ROW + 2.0 * STASH_GAP;

/// 正在预览标签里看的一条储藏。
#[derive(Debug, Clone)]
pub(crate) struct StashView {
    /// 储藏的提交号。
    pub oid: String,
    /// 储藏的名字（`stash@{0}`），写在差异标签的标题里。
    pub name: String,
    /// 改动的文件，各自连同比较的两个版本：（文件，左边的版本，右边的版本）；未跟踪的文件左边是
    /// `None`。
    pub files: Vec<(CommitFile, Option<String>, String)>,
    /// 现在看的是第几个文件。
    pub at: usize,
}

/// 后台取回的一条储藏的改动文件，或者失败的原因。
pub(super) type StashFiles = Result<Vec<(CommitFile, Option<String>, String)>, String>;

impl App {
    /// 储藏一组展开时标题下面的各行：各条储藏，正在看的那条下面是它的文件。
    ///
    /// 列出全部储藏，超出 [`STASH_ROWS`] 的部分靠滚动看到（见 [`App::stash_scroll_max`]）。
    pub(crate) fn stash_lines(&self) -> Vec<StashLine> {
        let open = self.listed_stash_view();
        let mut lines = Vec::new();
        for (i, stash) in self.git.stashes.iter().enumerate() {
            lines.push(StashLine::Stash(i));
            let Some(view) = open.filter(|view| view.oid == stash.oid) else {
                continue;
            };
            let count = view.files.len();
            if count <= STASH_FILE_ROWS {
                lines.extend((0..count).map(StashLine::File));
                continue;
            }
            let room = STASH_FILE_ROWS - 1;
            let start = view.at.saturating_sub(room / 2).min(count - room);
            lines.extend((start..start + room).map(StashLine::File));
            lines.push(StashLine::MoreFiles(count - room));
        }
        lines
    }

    /// 储藏一组里露出各行的范围（标题下面那一块）；储藏一组没有显示时为 `None`。
    pub(crate) fn stash_rows_area(&self) -> Option<Rect> {
        self.commit_parts().stashes.map(stash_body)
    }

    /// 储藏一组展开时露得下的行数（行）：[`App::stash_open_rows`] 与 [`STASH_ROWS`] 里小的那个。
    pub(crate) fn stash_visible_rows(&self) -> usize {
        self.stash_open_rows().min(STASH_ROWS)
    }

    /// 储藏一组最多能滚动多远（逻辑像素）：露不下的行数乘以行高。
    pub(crate) fn stash_scroll_max(&self) -> f32 {
        let hidden = self
            .stash_open_rows()
            .saturating_sub(self.stash_visible_rows());
        hidden as f32 * COMMIT_ROW
    }

    /// 在储藏一组里按下：开始上下拖动，内容跟着指针走；松开时没移动过才按点击处理
    /// （见 [`Self::release_stash_pan`]）。
    pub(in crate::app) fn begin_stash_pan(&mut self, target: Option<CommitTarget>) {
        self.settle_glides();
        self.pointer.drag = crate::app::Drag::StashPan {
            start_y: self.pointer.mouse.1,
            scroll: self.git.commit.stash_scroll,
            target,
        };
    }

    /// 拖动储藏一组：指针移动多少，各行就跟着走多少，限制在两端之间。
    pub(in crate::app) fn drag_stash_pan(&mut self, start_y: f32, scroll: f32, y: f32) {
        let max = self.stash_scroll_max();
        self.git.commit.stash_scroll = (scroll - (y - start_y)).clamp(0.0, max);
        self.request_redraw();
    }

    /// 松开储藏一组里的拖动：移动不到 [`DRAG_SLOP`] 时按点击处理。
    pub(in crate::app) fn release_stash_pan(&mut self, start_y: f32, target: Option<CommitTarget>) {
        let y = self.pointer.mouse.1;
        if (y - start_y).abs() > DRAG_SLOP {
            return;
        }
        if let Some(target) = target {
            self.press_commit_stash(target);
        }
    }

    /// 储藏一组展开时，标题下面的行数（[`App::stash_lines`]）。
    pub(crate) fn stash_open_rows(&self) -> usize {
        self.stash_lines().len()
    }

    /// 储藏的预览标签还开着、文件没有收起时，在里面看的储藏：它的文件列在储藏一组里。
    pub(crate) fn listed_stash_view(&self) -> Option<&StashView> {
        if self.git.commit.stash_files_folded {
            return None;
        }
        self.previewed_stash()
    }

    /// 储藏的预览标签还开着时，在里面看的储藏。
    fn previewed_stash(&self) -> Option<&StashView> {
        let view = self.git.commit.stash_view.as_ref()?;
        let id = self.git.commit.stash_preview?;
        self.editor
            .documents
            .iter()
            .any(|d| d.id == id)
            .then_some(view)
    }

    /// 储藏一组的终点高度变化（展开、收起一条储藏的文件，或在别处关闭了预览标签，文件不再列出）
    /// 时，从当前显示的高度开始过渡。每帧绘制提交视图之前调用，预览标签无论在何处关闭都能察觉。
    ///
    /// 整个一组出现或消失（新增第一条储藏、删除最后一条）时不过渡。
    pub(crate) fn track_stash_height(&mut self) {
        let settled = self.stash_settled_height();
        let old = std::mem::replace(&mut self.git.commit.stash_settled, settled);
        if (settled - old).abs() < 0.5 || old == 0.0 || settled == 0.0 {
            return;
        }
        let from =
            (self.git.commit.stash_motion).map_or(old, |(from, clock)| clock.between(from, old));
        let clock = self.clock(crate::app::animation::TREE_ANIMATION);
        self.git.commit.stash_motion = clock.map(|clock| (from, clock));
    }

    /// 储藏一组占的高度（逻辑像素）：收着时是 [`STASH_CLOSED`]，展开时再加上
    /// [`App::stash_open_rows`] 行；展开、收起的途中按时钟过渡。没有储藏时为 0。
    pub(crate) fn stash_section_height(&self) -> f32 {
        let settled = self.stash_settled_height();
        (self.git.commit.stash_motion).map_or(settled, |(from, clock)| clock.between(from, settled))
    }

    /// 储藏一组不在过渡中时的高度（逻辑像素），见 [`App::stash_section_height`]。
    fn stash_settled_height(&self) -> f32 {
        if self.git.stashes.is_empty() {
            return 0.0;
        }
        if self.git.commit.stash_open {
            STASH_CLOSED + self.stash_visible_rows() as f32 * COMMIT_ROW
        } else {
            STASH_CLOSED
        }
    }

    /// 储藏一组标题上的箭头转到哪儿：0 朝右（收着），1 朝下（展开）。
    ///
    /// 按一组现在拉开了多少算，途中再点一下往回收时箭头和高度一起从当时的位置往回走。
    pub(crate) fn stash_chevron(&self) -> f32 {
        let span = self.stash_open_rows() as f32 * COMMIT_ROW;
        if span <= 0.0 {
            return 0.0;
        }
        ((self.stash_section_height() - STASH_CLOSED) / span).clamp(0.0, 1.0)
    }

    /// 点了储藏一组的标题：收起或展开，从现在的高度过渡过去。
    pub(super) fn toggle_stashes(&mut self) {
        let from = self.stash_section_height();
        self.git.commit.stash_open = !self.git.commit.stash_open;
        let clock = self.clock(crate::app::animation::TREE_ANIMATION);
        self.git.commit.stash_motion = clock.map(|clock| (from, clock));
        // 过渡已经从现在的高度开始，`track_stash_height` 不必再开始一次。
        self.git.commit.stash_settled = self.stash_settled_height();
        self.git.commit.scroll = self.git.commit.scroll.min(self.commit_max_scroll());
    }

    /// 在预览标签里打开第 `i` 条储藏：先在后台取它改动的文件，取回来以后打开第一个。
    ///
    /// 点击的是正在查看的储藏时不重新打开，只收起或展开它下面的文件。
    pub(super) fn show_stash(&mut self, i: usize) {
        let Some(stash) = self.git.stashes.get(i).cloned() else {
            return;
        };
        if self
            .previewed_stash()
            .is_some_and(|view| view.oid == stash.oid)
        {
            self.git.commit.stash_files_folded = !self.git.commit.stash_files_folded;
            return;
        }
        let sender = self.git.commit.results.0.clone();
        self.run_git_job(move |git, root| {
            let files = git.and_then(|git| stash_files(git, root, &stash));
            let _ = sender.send(Outcome::StashFiles(stash.oid, stash.name, files));
        });
    }

    /// 后台取回了储藏 `oid`（名字是 `name`）改动的文件：打开第一个；没有能看的文件时说明。
    pub(super) fn stash_files_arrived(&mut self, oid: String, name: String, files: StashFiles) {
        let files = match files {
            Ok(files) => files,
            Err(e) => return self.flash(e),
        };
        if files.is_empty() {
            return self.flash(t!("git-stash-no-files"));
        }
        self.git.commit.stash_files_folded = false;
        self.git.commit.stash_view = Some(StashView {
            oid,
            name,
            files,
            at: 0,
        });
        self.open_stash_file(0);
    }

    /// 储藏一组下面列出的一个文件的菜单里选了「只取这个文件」：先确认（会盖掉工作区里这个文件
    /// 尚未提交的改动），确认以后把它从储藏里取到工作区。
    pub(in crate::app) fn confirm_take_stash_file(
        &mut self,
        path: PathBuf,
        from: String,
        deleted: bool,
    ) {
        let name = winc_editor::display_name(&path).into_owned();
        let message = if deleted {
            t!("git-stash-take-deletion-confirm", name = name.as_str())
        } else {
            t!("git-stash-take-file-confirm", name = name.as_str())
        };
        if self.window.handle.is_some() {
            let (detail, button) = if deleted {
                (
                    t!("git-stash-take-deletion-confirm-detail"),
                    t!("git-stash-take-deletion-confirm-button"),
                )
            } else {
                (
                    t!("git-stash-take-file-confirm-detail"),
                    t!("git-stash-take-file-confirm-button"),
                )
            };
            self.show_confirm(&message, detail, button, move |ok| {
                ok.then(|| UserEvent::GitTakeFile(from.clone(), path.clone(), deleted))
            });
            return;
        }
        // 没有窗口（测试中）时直接取出。
        self.take_stash_file(from, path, deleted);
    }

    /// 把储藏里的 `path` 取到工作区和暂存区（`deleted` 时是把它从两处一并删掉），完成后重新读取
    /// 状态。储藏本身不变，其余的文件也不动。
    pub(in crate::app) fn take_stash_file(&mut self, from: String, path: PathBuf, deleted: bool) {
        let name = winc_editor::display_name(&path).into_owned();
        let what = t!("git-op-stash-take-file", name = name.as_str());
        self.git_operate(what, move |git, root| {
            git.take_file(root, &from, &path, deleted)
        });
    }

    /// 右键点在提交视图的储藏一组里：点在一条储藏上时是它的菜单，点在它下面列出的一个文件上时是
    /// 那个文件的菜单。
    pub(in crate::app) fn stash_context_target(&self, x: f32, y: f32) -> Option<MenuTarget> {
        if !self.commit_shown() || self.git.commit.hits_generation != self.git.generation {
            return None;
        }
        match crate::layout::hit(&self.hits.commit, x, y)? {
            CommitTarget::Stash(i) | CommitTarget::StashApply(i) | CommitTarget::StashMenu(i) => {
                Some(MenuTarget::StashRow(i))
            }
            CommitTarget::StashFile(at) => self.stash_file_target(at),
            _ => None,
        }
    }

    /// 储藏一组里第 `at` 个文件的菜单目标：路径、取它用的版本，以及它在储藏里是不是被删掉了。
    fn stash_file_target(&self, at: usize) -> Option<MenuTarget> {
        let view = self.listed_stash_view()?;
        let (file, _, from) = view.files.get(at)?;
        Some(MenuTarget::StashFile {
            at,
            path: file.path.clone(),
            from: from.clone(),
            deleted: file.change == winc_editor::git::Change::Deleted,
        })
    }

    /// 在储藏的预览标签里打开正在看的储藏的第 `at` 个文件。
    pub(in crate::app) fn open_stash_file(&mut self, at: usize) {
        let Some(view) = self.git.commit.stash_view.as_mut() else {
            return;
        };
        let Some((file, old, new)) = view.files.get(at).cloned() else {
            return;
        };
        view.at = at;
        let name = winc_editor::display_name(&file.path).into_owned();
        let title = format!("{name} · {}", view.name);
        let sender = self.git.commit.results.0.clone();
        self.run_git_job(move |git, root| {
            let diff = git.and_then(|git| {
                crate::app::git::log::commit_diff(git, root, old.as_deref(), &new, &file)
            });
            let _ = sender.send(Outcome::StashDiff(title, diff.map(Box::new)));
        });
    }

    /// 后台准备好了储藏里一个文件的差异：在储藏的预览标签里换上它。
    pub(super) fn stash_diff_arrived(
        &mut self,
        title: String,
        view: Result<Box<winc_editor::diff_view::DiffView>, String>,
    ) {
        match view {
            Ok(view) => {
                let index = self
                    .editor
                    .show_diff_in(self.git.commit.stash_preview, &title, *view);
                self.git.commit.stash_preview = Some(self.editor.documents[index].id);
                self.relayout();
                self.clamp_scroll();
                self.land_diff_file();
            }
            Err(e) => {
                self.diff.land = None;
                self.flash(e);
            }
        }
    }

    /// 当前标签是不是储藏的预览标签，而且还在看那条储藏。
    fn viewing_stash(&self) -> Option<&StashView> {
        let active: Option<DocumentId> = self.editor.active().map(|d| d.id);
        let view = self.git.commit.stash_view.as_ref()?;
        (active.is_some() && active == self.git.commit.stash_preview).then_some(view)
    }

    /// 储藏的预览标签里，同一条储藏中的下一个（`forward`）或上一个文件：（从 0 起的序号，文件名，
    /// 文件总数）；当前标签不是储藏的预览，或者没有更多文件时为 `None`。
    pub(in crate::app) fn next_stash_file(&self, forward: bool) -> Option<(usize, String, usize)> {
        let view = self.viewing_stash()?;
        let next = if forward {
            view.at + 1
        } else {
            view.at.checked_sub(1)?
        };
        let (file, ..) = view.files.get(next)?;
        let name = winc_editor::display_name(&file.path).into_owned();
        Some((next, name, view.files.len()))
    }

    /// 在储藏的预览标签里打开下一个（`forward`）或上一个文件，返回是否已打开。
    pub(in crate::app) fn step_stash_file(&mut self, forward: bool) -> bool {
        let Some((next, name, total)) = self.next_stash_file(forward) else {
            return false;
        };
        self.open_stash_file(next);
        let (index, name) = (next + 1, name.as_str());
        self.flash(if forward {
            t!("git-next-file", name = name, index = index, total = total)
        } else {
            t!(
                "git-previous-file",
                name = name,
                index = index,
                total = total
            )
        });
        true
    }
}

/// 储藏 `stash` 改动的文件，连同比较的两个版本。
///
/// 已跟踪的文件比较储藏时的 `HEAD`（第一个父提交）和储藏本身；一起收进去的未跟踪文件在第三个父
/// 提交里，左边为空。
fn stash_files(
    git: &winc_editor::git::Git,
    root: &std::path::Path,
    stash: &Stash,
) -> Result<Vec<(CommitFile, Option<String>, String)>, String> {
    let cancel = winc_editor::format::Cancel::default();
    let base = stash.parents.first().cloned();
    let tracked = git.commit_files(root, base.as_deref(), &stash.oid, &cancel)?;
    let mut files: Vec<_> = (tracked.into_iter())
        .map(|file| (file, base.clone(), stash.oid.clone()))
        .collect();
    if let Some(untracked) = stash.parents.get(2) {
        let added = git.commit_files(root, None, untracked, &cancel)?;
        files.extend(
            added
                .into_iter()
                .map(|file| (file, None, untracked.clone())),
        );
    }
    Ok(files)
}
