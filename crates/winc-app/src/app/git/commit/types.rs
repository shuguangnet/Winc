//! 提交视图的数据结构：列表行、分组、勾选缓存、动画、排序方式、点击目标与排版几何。

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::{Deref, Range};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use winc_editor::git::{Entry, Status};
use winc_i18n::t;
use winc_render::Rect;

use super::{COMMIT_ROW, OtherRow, others};

/// 绘制时量好的几段文字的宽度（逻辑像素），供 [`crate::app::App::commit_parts`] 排版：布局时不量文字。
///
/// 第一次绘制之前都是 0，按钮按最窄排；绘制时先量好再排版，同一帧里就用上了。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct CommitLabels {
    /// 「修改上次提交」：勾选框连同后面的文字。
    pub amend: f32,
    /// 提交按钮上的文字，连同前面的对勾。
    pub button: f32,
    /// 「上一次提交」那一栏的「撤销」。
    pub undo: f32,
    /// 「提交并推送…」按钮上的文字。
    pub push: f32,
}

/// 改动列表中悬停行的文件名溢出信息。
///
/// 文件名被截短或与右端的状态字母重叠时，悬停时显示全名并隐藏状态字母；超出侧栏的部分绘制
/// 在编辑区之上。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CommitOverflow {
    /// 列表里的第几行（[`crate::app::App::commit_rows`] 的下标）。
    pub row: usize,
    /// 这一行的上沿（逻辑像素；固定在顶部的行用固定的位置）。
    pub y: f32,
    /// 伸出侧栏的那一段；全名在侧栏里写得下（只是要让开状态字母）时为 `None`。
    pub spill: Option<Rect>,
    /// 文件行跟在全名后面的「打开文件」「回滚」两个按钮；目录行为 `None`。
    pub actions: Option<(Rect, Rect)>,
}

/// 改动列表里一行名字的过渡状态的键（见 `TreeLabels`）：滚动、展开收起前后不变。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum CommitName {
    /// 一个目录：同一个目录可以同时出现在几组里，连同组区分。
    Dir(CommitGroup, PathBuf),
    /// 一个文件（`Status::entries` 里的路径）。
    File(PathBuf),
}

/// 改动列表里的一组。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommitGroup {
    /// 已跟踪文件的改动和已暂存的新文件，默认勾上。
    Changes,
    /// 合并、拣选、变基时有冲突的文件。
    ///
    /// 冲突须先于其他改动处理，因此单独成组并排在最上方，不与普通改动混排。
    Conflicts,
    /// 未跟踪的文件（状态字母 U）：默认不勾，只有勾上才随提交加入仓库。
    Untracked,
    /// 暂存区中有内容的文件；只在暂存区视图中有。只暂存了一部分的文件同时列在「更改」一组。
    Staged,
}

impl CommitGroup {
    /// 各组在列表中自上而下的顺序。
    pub const ALL: [CommitGroup; CommitGroup::COUNT] = [
        CommitGroup::Conflicts,
        CommitGroup::Staged,
        CommitGroup::Changes,
        CommitGroup::Untracked,
    ];
    /// 组的个数。
    pub const COUNT: usize = 4;

    /// 组的标题。
    pub fn title(self) -> &'static str {
        match self {
            CommitGroup::Changes => t!("git-commit-group-changes"),
            CommitGroup::Conflicts => t!("git-commit-group-conflicts"),
            CommitGroup::Untracked => t!("git-commit-group-untracked"),
            CommitGroup::Staged => t!("git-commit-group-staged"),
        }
    }

    pub(super) fn index(self) -> usize {
        self as usize
    }

    /// `entry` 是否属于这一组；`staging` 表示是否在暂存区视图中。
    ///
    /// 暂存区视图中「更改」一组只含有未暂存改动的文件，全部暂存的文件只在「已暂存的更改」中；
    /// 勾选式下没有「已暂存的更改」，已跟踪文件的改动都在「更改」中。
    pub fn holds(self, entry: &Entry, staging: bool) -> bool {
        let plain = !entry.conflicted();
        match self {
            CommitGroup::Conflicts => entry.conflicted(),
            CommitGroup::Staged => staging && plain && entry.staged.is_some(),
            CommitGroup::Changes if staging => {
                plain && !entry.untracked() && entry.unstaged.is_some()
            }
            CommitGroup::Changes => plain && !entry.untracked(),
            CommitGroup::Untracked => plain && entry.untracked(),
        }
    }
}

/// 列表里的一行。
///
/// 组下面按目录排成树：先是目录，再是文件，各自按名字排；只有一个子目录、没有文件的几级合成
/// 一行（「src/core」）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitRow {
    /// 组的标题行。
    Group(CommitGroup),
    /// 一个目录：所在的组、目录（相对仓库根目录）、显示的名字（合起来的几级）、缩进的层数
    /// （组下面第一层是 0）。
    Dir {
        group: CommitGroup,
        dir: PathBuf,
        label: String,
        depth: usize,
    },
    /// 一个文件：它在 `Status::entries` 里的下标、缩进的层数。
    File { index: usize, depth: usize },
}

impl CommitRow {
    /// 这一行在树里的层级：组是 0，组下面第一层（目录、文件）是 1，依此类推。
    ///
    /// 固定在列表顶部的行按它排：第 n 层固定在顶部往下第 n 行。
    pub fn level(&self) -> usize {
        match self {
            CommitRow::Group(_) => 0,
            CommitRow::Dir { depth, .. } | CommitRow::File { depth, .. } => depth + 1,
        }
    }

    /// 是不是能装别的行的（组、目录）。
    pub(super) fn is_container(&self) -> bool {
        !matches!(self, CommitRow::File { .. })
    }
}

/// 排好的改动列表：各行，和每组、每个目录下面有哪些文件（画勾选框、点勾选框时用，不必每行
/// 把所有改动翻一遍）。
///
/// 按状态和收起、平铺的样子排一次，缓存到它们变了为止（见 [`crate::app::App::commit_rows`]）；解引用成
/// 各行。
#[derive(Debug, Default)]
pub(crate) struct CommitRows {
    pub(super) rows: Vec<CommitRow>,
    /// 各组、各目录下面的文件（`Status::entries` 的下标）：一组、一个目录（收起的也算）下面的
    /// 文件在这里连成一段，它里面的目录是其中的一小段。
    pub(super) members: Vec<usize>,
    /// 每组的文件在 `members` 里的那一段（下标是 `CommitGroup as usize`）。
    pub(super) groups: [Range<usize>; CommitGroup::COUNT],
    /// 列表里每个目录行的文件在 `members` 里的那一段：下标同上，键是相对仓库根目录的目录。
    pub(super) dirs: [HashMap<PathBuf, Range<usize>>; CommitGroup::COUNT],
    /// 组、目录行的勾选框（见 [`CommitRows::members_state`]）：（勾选的版本，按那一段在
    /// `members` 里的位置记下的结果）。
    pub(super) states: RefCell<MemberStates>,
}

/// [`CommitRows::members_state`] 的缓存：勾选的版本，以及按那一段在 `members` 里的
/// （起点，长度）记下的勾选状态。
type MemberStates = (u64, HashMap<(usize, usize), Option<bool>>);

impl CommitRows {
    /// 返回第 `row` 行所在的组：该行或它之前最近的组标题行；没有这一行时为 `None`。
    pub fn group_of(&self, row: usize) -> Option<CommitGroup> {
        self.rows.get(..=row)?.iter().rev().find_map(|r| match r {
            CommitRow::Group(group) => Some(*group),
            _ => None,
        })
    }

    /// 组 `group` 里的文件（`Status::entries` 的下标）。
    pub fn group(&self, group: CommitGroup) -> &[usize] {
        &self.members[self.groups[group.index()].clone()]
    }

    /// 组 `group` 里目录 `dir`（相对仓库根目录）下面的文件；列表里没有这个目录行时为空。
    pub fn dir(&self, group: CommitGroup, dir: &Path) -> &[usize] {
        self.dirs[group.index()]
            .get(dir)
            .map_or(&[], |r| &self.members[r.clone()])
    }

    /// `members`（本表的 [`Self::group`]、[`Self::dir`] 给出的一段）的勾选状态，含义同
    /// [`super::CommitState::members_state`]；`checks` 是勾选的版本（[`super::CommitState::checks`]）。
    ///
    /// 组、目录下面可能有上万个文件，每帧都要画它们的勾选框：按位置记下结果，勾选变了、或者
    /// 重新排了各行（换成另一张表）时才重新算。
    pub fn members_state(
        &self,
        members: &[usize],
        checks: u64,
        compute: impl FnOnce() -> Option<bool>,
    ) -> Option<bool> {
        if members.is_empty() {
            return compute();
        }
        let offset = (members.as_ptr() as usize - self.members.as_ptr() as usize)
            / std::mem::size_of::<usize>();
        let mut states = self.states.borrow_mut();
        if states.0 != checks {
            *states = (checks, HashMap::new());
        }
        *states
            .1
            .entry((offset, members.len()))
            .or_insert_with(compute)
    }

    /// 排下一层已经排好序的文件 `files`（下标、相对路径）：每个一行，深 `depth` 层。
    pub(super) fn push_files(&mut self, files: &[(usize, &Path)], depth: usize) {
        self.members.extend(files.iter().map(|&(index, _)| index));
        self.rows.extend(
            files
                .iter()
                .map(|&(index, _)| CommitRow::File { index, depth }),
        );
    }
}

impl Deref for CommitRows {
    type Target = [CommitRow];

    fn deref(&self) -> &[CommitRow] {
        &self.rows
    }
}

/// 列表滚动时固定在顶部的一行。
///
/// 已滚出视口的行所属的组和各级目录逐层固定在顶部，下一组或下一个目录到达时将其向上推出。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CommitSticky {
    /// 它在列表各行（[`crate::app::App::commit_rows`]）里的下标。
    pub row: usize,
    /// 第几层，固定在列表顶部往下第几行。
    pub level: usize,
    /// 画在哪里（被推上去时比固定的位置高）。
    pub rect: Rect,
}

/// 刷新前后的状态 `old`、`new` 里改动的文件或者它们的暂存/工作区状态是否不同：只是文件内容变了
/// （保存）时相同。
pub(crate) fn entries_changed(old: &Status, new: &Status) -> bool {
    old.entries.len() != new.entries.len()
        || (old.entries.iter().zip(&new.entries)).any(|(a, b)| {
            a.path != b.path
                || a.staged != b.staged
                || a.unstaged != b.unstaged
                || a.shown() != b.shown()
        })
}

/// 刷新以后改动列表的过渡：离开的行（提交了、回滚了）原地淡出，新出现的行淡入，留下的行从原来
/// 的位置移到新位置；列表空了或者从空变成有时，空状态同时淡入、淡出。
///
/// 离开的行在新的状态里已经没有了，按刷新前的状态和各行绘制。
pub(crate) struct CommitShift {
    /// 刷新前的仓库状态。
    pub status: Status,
    /// 刷新前的各行。
    pub rows: Rc<CommitRows>,
    /// 刷新前列表滚动的距离（逻辑像素）。
    pub scroll: f32,
    pub clock: crate::app::animation::Clock,
}

/// 组、目录展开或收起的动画。
///
/// 子行从父行下方逐渐显示或收回，其后的各行随之移动。
#[derive(Debug, Clone)]
pub(crate) struct CommitMotion {
    /// 动画期间画的各行：展开时是展开以后的，收起时是收起之前的。
    pub rows: Rc<CommitRows>,
    /// 展开或收起的组、目录在 `rows` 里的下标。
    pub parent: usize,
    /// 露出或收回的行数。
    pub count: usize,
    /// 是展开；`false` 是收起。
    pub expanding: bool,
    /// 这一段动画的时钟。
    pub(in crate::app) clock: crate::app::Clock,
}

impl CommitMotion {
    /// 子行在 `rows` 里的范围 `[起, 止)`。
    pub fn block(&self) -> (usize, usize) {
        (self.parent + 1, self.parent + 1 + self.count)
    }

    /// 还没露出来的子行占的高度（逻辑像素）：下面的行往上收该距离。
    pub fn hidden(&self) -> f32 {
        let eased = self.clock.progress();
        let reveal = if self.expanding { eased } else { 1.0 - eased };
        (1.0 - reveal) * self.count as f32 * COMMIT_ROW
    }
}

/// 多仓库时某个仓库一节或其中一组的展开、收起动画。动画期间按行数较多的状态画；其他仓库的
/// 子行位置不动，只露出已露出的一段，下面的行随之移动；当前仓库一节则是改动列表的高度随之变化。
#[derive(Debug, Clone)]
pub(crate) struct FoldMotion {
    /// 动画期间画的各行：展开时是展开以后的，收起时是收起之前的。
    pub rows: Vec<OtherRow>,
    /// 展开或收起的标题行在 `rows` 里的下标。
    pub parent: usize,
    /// 露出或收回的行数；当前仓库一节为 0，收回的是它的改动列表。
    pub count: usize,
    /// 露出或收回的总高度（逻辑像素）。
    pub height: f32,
    /// 是展开；`false` 是收起。
    pub expanding: bool,
    /// 这一段动画的时钟。
    pub(in crate::app) clock: crate::app::Clock,
}

impl FoldMotion {
    /// 已露出的比例：0 是完全收起，1 是完全展开。
    pub fn reveal(&self) -> f32 {
        let eased = self.clock.progress();
        if self.expanding { eased } else { 1.0 - eased }
    }

    /// 还没露出来的高度（逻辑像素）。
    pub fn hidden(&self) -> f32 {
        (1.0 - self.reveal()) * self.height
    }
}

/// 提交视图里可以点的地方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitTarget {
    /// 标题行的「Git 日志」：打开日志标签。
    Log,
    /// 标题行的「刷新」：重新读取状态和分支，读取期间图标旋转。
    Refresh,
    /// 标题行的「⋯」：弹出同步、储藏、远程仓库和显示方式的菜单。
    More,
    /// 与提交按钮并排的「提交并推送…」：确认后提交并推送。
    CommitPush,
    /// 暂存区视图中文件行行尾的「＋」（第二项为真）或「−」：暂存或取消暂存第几个文件。
    StageFile(usize, bool),
    /// 暂存区视图中组标题行尾的「＋」或「−」：暂存这一组的全部文件，「已暂存的更改」一组是
    /// 全部取消暂存。
    GroupStage(CommitGroup),
    /// 另一个仓库的标题行（它在仓库列表中的下标）：切换到该仓库。
    OtherRepository(usize),
    /// 另一个仓库中的文件（仓库的下标，文件在其状态各项中的下标）：切换过去并选中该文件。
    OtherFile(usize, usize),
    /// 另一个仓库标题行上的分支：切换到该仓库并展开它的分支选择面板。
    OtherBranch(usize),
    /// 打开的目录不是仓库时的「初始化仓库」。
    InitRepository,
    /// 打开的目录不是仓库时的「克隆仓库…」。
    CloneRepository,
    /// 勾选式下另一个仓库标题行的勾选框：勾上或取消该仓库的全部文件。
    OtherGroupCheck(usize),
    /// 勾选式下另一个仓库中文件的勾选框（仓库的下标，文件的下标）。
    OtherCheck(usize, usize),
    /// 暂存区视图中另一个仓库已暂存的文件（仓库的下标，文件的下标）：切换过去并选中该文件。
    OtherStagedFile(usize, usize),
    /// 暂存区视图中另一个仓库文件行行尾的「＋」（第三项为真）或「−」：暂存或取消暂存该文件。
    OtherStage(usize, usize, bool),
    /// 另一个仓库里的一组的标题行（仓库的下标，组）：折叠或展开这一组；暂存区视图中悬停时行尾
    /// 显示暂存、取消暂存整组的按钮。
    OtherGroup(usize, others::OtherGroup),
    /// 勾选式下另一个仓库里一组标题的勾选框：勾上或取消这一组的全部文件。
    OtherGroupFiles(usize, others::OtherGroup),
    /// 另一个仓库标题行左端的折叠箭头：折叠或展开这个仓库的一节。
    OtherFold(usize),
    /// 多仓库时当前仓库的标题行：点击不做任何操作。
    CurrentRepository,
    /// 多仓库时当前仓库标题行左端的折叠箭头：折叠或展开它的一节。
    CurrentFold,
    /// 多仓库时「没有更改的仓库」一组的标题行：展开或收起这一组。
    QuietRepositories,
    /// 勾选式下当前仓库标题行的勾选框：勾上或取消当前仓库的全部文件。
    CurrentCheck,
    /// 暂存区视图中另一个仓库分组行行尾的「＋」（第二项为真）或「−」：暂存或取消暂存整组。
    OtherGroupStage(usize, bool),
    /// 组标题行：收起或展开。
    Group(CommitGroup),
    /// 组标题的勾选框：整组勾上或取消。
    GroupCheck(CommitGroup),
    /// 悬停组标题时右端的「回滚」按钮：回滚这一组里勾上的文件（先确认）。
    GroupRevert(CommitGroup),
    /// 储藏一组的标题：收起或展开。
    Stashes,
    /// 第几条储藏（`GitState::stashes` 的下标）那一行：在预览标签里看它的改动。
    Stash(usize),
    /// 正在看的储藏下面列出的一个文件（`StashView::files` 的下标）：在预览标签里看它。
    StashFile(usize),
    /// 悬停储藏那一行时右端的「应用」。
    StashApply(usize),
    /// 悬停储藏那一行时右端的「⋯」：应用、应用并删除、删除。
    StashMenu(usize),
    /// 工作树一组的标题：收起或展开。
    Worktrees,
    /// 悬停工作树一组的标题时右端的「新建工作树」。
    WorktreeNew,
    /// 第几个工作树（[`crate::app::App::worktree_places`] 的下标）那一行：打开该工作树。
    WorktreeRow(usize),
    /// 悬停工作树一行时右端的「⋯」，参数同 [`CommitTarget::WorktreeRow`]。
    WorktreeMenu(usize),
    /// 工作树太多时最后一行的「另有 N 个工作树」：展开工作树选择器。
    WorktreeMore,
    /// 目录那一行：收起或展开（参数是它在绘制时的行号，即 [`crate::app::App::commit_rows`] 的下标）。
    Dir(usize),
    /// 目录的勾选框：目录下的文件全部勾上或取消（参数同 [`CommitTarget::Dir`]）。
    DirCheck(usize),
    /// 悬停目录行时右端的「回滚」按钮：回滚该目录下的文件（先确认），参数同 [`CommitTarget::Dir`]。
    DirRevert(usize),
    /// 暂存区视图中悬停目录行时右端的「＋」（第二项为真）或「−」：暂存或取消暂存该目录下的全部文件。
    DirStage(usize, bool),
    /// 文件的勾选框，参数是 `Status::entries` 的下标。
    Check(usize),
    /// 文件那一行：选中并预览差异；双击打开文件。
    File(usize),
    /// 悬停文件行时右端的「打开文件」按钮，参数同 [`CommitTarget::File`]。
    OpenFile(usize),
    /// 悬停文件行时右端的「回滚」按钮，参数同 [`CommitTarget::File`]。
    RevertFile(usize),
    /// Git 上下文一行的仓库按钮（打开的目录中有多个仓库时才有）：展开仓库选择器。
    Location,
    /// Git 上下文的分支行：展开分支选择器。
    Branch,
    /// 分支行右端的同步控件：同步、发布或获取，见 [`crate::app::App::git_sync`]。
    Sync,
    /// 拆分按钮左边的「提交」。
    Commit,
    /// 拆分按钮右边的下拉箭头：弹「提交并推送」；卡片太窄、放不下勾选框时还有「修改上一次提交」。
    CommitMenu,
    /// 提交卡片底行左边的「修改上次提交」勾选框和文字。
    Amend,
    /// 没有更改时空状态下面的按钮：推送、拉取或发布，见 [`crate::app::App::git_sync`]。
    NextStep,
    /// 「上一次提交」那一栏的「撤销」。
    Undo,
    /// 「上一次提交」那一栏的「⋯」。
    LastCommit,
    /// 合并、变基暂停时说明条右端的「中止」按钮。
    Abort,
    /// 提交图标题行左侧的「v 图表」：折叠或展开提交图区域。
    GraphToggle,
    /// 提交图里的第几行（提交图里 `Commit` 的下标）：展开或收起它改动的文件（见 [`crate::app::git::graph`]）。
    GraphRow(usize),
    /// 提交图里第几个提交行右端的「在日志中查看」按钮：在主日志标签中定位该提交。
    GraphOpenLog(usize),
    /// 提交图里第几个提交行右端的「复制提交号」按钮：复制该提交的完整提交号。
    GraphCopyOid(usize),
    /// 提交图展开的文件列表里的第几个文件（`GraphPart::files` 的下标）：直接打开该文件在此提交中的差异。
    GraphFile(usize),
    /// 提交图展开的文件列表中第几个文件行右端的「打开文件」按钮：打开工作区中的当前文件。
    GraphFileOpen(usize),
    /// 提交图标题行右端的「定位到 HEAD」（`◎`）：滚动到当前 `HEAD` 提交。
    GraphFocusHead,
    /// 提交图标题行右端的「获取」：获取远程更新。
    GraphFetch,
    /// 提交图标题行右端的「拉取」：从上游拉取。
    GraphPull,
    /// 提交图标题行右端的「推送」：推送到上游（`HEAD` 已游离时先问推到哪）。
    GraphPush,
    /// 提交图标题行右端的「刷新」（`↻`）：重新读取提交图。
    GraphRefresh,
    /// 提交图标题行右端的「更多 / 完整历史」（`⋯`）：打开完整 Git 提交历史标签页。
    GraphMore,
    /// 提交图标题行的对比控件：弹出换基准的菜单。
    GraphCompare,
    /// 提交图上沿的分隔线：按住上下拖动，调整图占的高度。
    GraphDivider,
    /// 提交图里的空白处（最后一行下面的部分）：按住同样能上下拖动，不用先找一行。
    GraphBlank,
    /// 固定在列表顶部的组或目录：列表先滚到它真正那一行停在固定的位置（画面不动），再收起或展开（参数是下标和层级）。
    Sticky(usize, usize),
}

/// 提交视图各组里文件的排序方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CommitSort {
    /// 按路径：同一个目录的文件排在一起。
    #[default]
    Path,
    /// 按文件名，不看所在的目录。
    Name,
    /// 按更改的类型（状态字母），同一类型内按路径。
    Status,
}

impl CommitSort {
    /// 全部排序方式，顺序就是菜单里的顺序。
    pub const ALL: [CommitSort; 3] = [CommitSort::Path, CommitSort::Name, CommitSort::Status];

    /// 记在状态文件里的写法；按路径是默认，记作空串。
    pub fn key(self) -> &'static str {
        match self {
            CommitSort::Path => "",
            CommitSort::Name => "name",
            CommitSort::Status => "status",
        }
    }

    /// 从状态文件里的写法读出；认不出的按路径。
    pub fn from_key(key: &str) -> CommitSort {
        let known = CommitSort::ALL.into_iter().find(|sort| sort.key() == key);
        known.unwrap_or_default()
    }
}

/// 提交视图的几何。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CommitParts {
    /// 标题行（左边是项目和分支的上下文栏，右端是工具按钮）。
    pub header: Rect,
    /// 标题行右端的「Git 日志」「刷新」「⋯」三个按钮。
    pub tools: [(Rect, CommitTarget); 3],
    /// 合并、变基等操作暂停时改动列表上方的说明条及其右端的「中止」按钮。
    ///
    /// 没有进行中的操作时为 `None`。
    pub banner: Option<(Rect, Rect)>,
    /// 视图底部的提交卡片：提交信息和底行的勾选框、按钮都在里面。
    ///
    /// 不显示（见 [`crate::app::App::commit_compose_shown`]）时为 `None`，下面几项都是空矩形。
    pub card: Option<Rect>,
    /// 提交信息区的外框，在卡片的上部。
    pub message: Rect,
    /// 框高过渡结束后提交信息区的外框；文字按它排版，过渡途中只绘制在 `message` 之内。
    pub message_settled: Rect,
    /// 卡片底行左边的「修改上次提交」勾选框和文字；卡片太窄时收进按钮的菜单，为 `None`。
    pub amend: Option<Rect>,
    /// 拆分按钮左边的「提交」。
    pub commit: Rect,
    /// 拆分按钮右边的下拉箭头。
    pub commit_menu: Rect,
    /// 与提交按钮并排的「提交并推送…」按钮；底行放不下、或者这时不能推送时为 `None`，推送在
    /// 下拉菜单中。
    pub push: Option<Rect>,
    /// 位置选择面板对着的区域：标题行里内容的范围；只有一个位置时没有。
    pub location: Option<Rect>,
    /// 分支行。
    pub branch: Rect,
    /// 「上一次提交」那一栏（整个侧栏宽，在视图底部或提交卡片上面）及其右端的「撤销」「⋯」两个
    /// 按钮。
    ///
    /// 上一次提交不能撤销（已经推送、不在分支上、有进行中的操作）时为 `None`。
    pub undo: Option<(Rect, Rect, Rect)>,
    /// 储藏一组（整个侧栏宽，上沿一条分隔线），在改动列表下面；没有储藏时为 `None`。
    pub stashes: Option<Rect>,
    /// 工作树一组，在改动列表和储藏之间；当前仓库只有一个工作树时为 `None`。
    pub worktrees: Option<Rect>,
    /// 改动列表。
    pub list: Rect,
    /// 提交图（改动列表下面、储藏上面，整个侧栏宽）；没有提交或者侧栏太矮时为 `None`。
    pub graph: Option<Rect>,
}
