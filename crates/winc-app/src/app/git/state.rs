//! Git 模块的界面线程状态结构与共享工具封装。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use ropey::Rope;
use winc_editor::diff_view::DiffView;
use winc_editor::format::Cancel;
use winc_editor::git::{Branch, Change, Git, Stash, Status, Unavailable, Worktree};
use winc_editor::shell::Shell;
use winc_editor::{DocumentId, Encoding};
use winc_i18n::t;

use super::{blame, branch, changes, commit, fetch, form, graph, location_picker, log, merge};

/// 界面线程与各后台任务共用的 [`Git`]。
///
/// 第一个找到 `git` 的任务写入结果，之后的任务直接使用；未找到时不写入，由下一个任务重新查找
/// （用户可能刚安装 `git`）。每个 [`Git`] 各自记录仓库的信任状态，共用同一个实例才能使用户对
/// 仓库配置的信任决定在各任务之间一致。
#[derive(Debug, Clone, Default)]
pub(crate) struct SharedGit(Arc<Mutex<Option<Git>>>);

impl SharedGit {
    /// 返回已找到的 `git`；尚未查找或未找到时返回 `None`。
    pub(crate) fn get(&self) -> Option<Git> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// 返回共用的 `git`，尚未找到时就地查找并写入；两个任务同时找到时保留先写入的结果。
    ///
    /// 第一次查找需要等待读取用户 shell 的环境，因此只在后台线程上调用。
    pub(super) fn resolve(&self, shell: &Shell, program: Option<&str>) -> Result<Git, Unavailable> {
        if let Some(git) = self.get() {
            return Ok(git);
        }
        let found = Git::find_with(shell, program)?;
        let mut slot = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(slot.get_or_insert(found).clone())
    }

    /// 记录已找到的 `git`，之后不再查找。
    pub(in crate::app) fn keep(&self, git: Git) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(git);
    }

    /// 清除已找到的 `git`（关闭目录或 Git 模块时），下次使用时重新查找。
    pub(in crate::app) fn forget(&self) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// 返回后台任务找不到 `git` 时向用户显示的原因。
pub(in crate::app) fn unusable(e: &Unavailable) -> String {
    t!("git-unusable", error = e.to_string())
}

/// 一次后台刷新的结果。
pub(super) struct Outcome {
    /// 刷新的仓库；结果送达时当前仓库已更换则丢弃。
    pub(super) root: PathBuf,
    /// 读取到的状态；失败时为原因。
    pub(super) status: Result<Status, String>,
    /// 失败的原因是否为没有可用的 `git`。
    pub(super) missing: bool,
    /// 分支、储藏与工作树列表。
    ///
    /// 状态读取失败，或本次只刷新状态（[`crate::app::App::git_refresh_status`]）时不读取。
    pub(super) refs: Option<Refs>,
}

/// 与状态一起刷新的列表；这些列表只随 `.git` 中的内容变化。
pub(super) struct Refs {
    /// 本地和远程分支，本地的在前。
    pub(super) branches: Vec<Branch>,
    /// 各个标签的名字，最近创建的在前。
    pub(super) tags: Vec<String>,
    /// 仓库配置的远程名。
    pub(super) remotes: Vec<String>,
    /// 仓库配置的远程仓库和地址。
    pub(super) remote_list: Vec<winc_editor::git::Remote>,
    /// 发布当前分支时使用的远程（[`winc_editor::git::Git::publish_target`]）；没有远程时为
    /// `None`。
    pub(super) publish_remote: Option<String>,
    /// 仓库的公共 Git 目录；取不到时为 `None`。
    pub(super) common_dir: Option<PathBuf>,
    /// 储藏，最新的在前。
    pub(super) stashes: Vec<Stash>,
    /// 最近切换到过的分支名，最近的在前。
    pub(super) recent_branches: Vec<String>,
    /// 工作树列表；读取失败（`git` 版本过旧、超时）时为 `None`，保留此前的列表，
    /// 否则被占用的分支、窗口标题和新建工作树的存放位置都会出错。
    pub(super) worktrees: Option<Vec<Worktree>>,
    /// 仓库所在系统中用户的主目录（[`winc_editor::wsl::home_dir`]）。
    pub(super) home: Option<PathBuf>,
    /// 仓库所在系统中 `git` 的主、次版本号；读不出时为 `None`。
    pub(super) version: Option<(u32, u32)>,
    /// 最近提交的作者和自己写的提交信息（见 [`Git::recent_history`]）；这次没有读时为 `None`。
    pub(super) history: Option<winc_editor::git::RecentHistory>,
}

/// 差异的打开方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum Target {
    /// 新建差异标签（或替换同名的差异标签）并切换到该标签。
    Open,
    /// 提交视图的预览标签，参数为请求序号。只采用最新的请求，快速切换选择时，
    /// 先发出而后送达的结果被丢弃。
    Preview(u64),
    /// 原地更新已打开的差异标签，不切换标签，不改变滚动位置。
    Refresh(DocumentId),
}

/// 后台准备好的差异。
pub(super) struct Prepared {
    /// 进行比较的仓库；之后更换了目录时丢弃结果。
    pub(super) root: PathBuf,
    /// 比较的文件。
    pub(super) path: PathBuf,
    /// 比较时 `HEAD` 的提交号，左侧内容取自该提交。
    pub(super) oid: Option<String>,
    pub(super) target: Target,
    /// 比较结果；无法比较时为原因（二进制、过大、无法读取）。
    pub(super) view: Result<DiffView, String>,
}

/// 发起比较时编辑器中该文件的快照。
pub(super) struct Snapshot {
    /// 发起比较时的文本。
    pub(super) text: Rope,
    /// 发起比较时的文档版本号，记进差异视图（`DiffView::source`）。
    pub(super) revision: u64,
    /// 文档的编码，左侧内容按该编码解码。
    pub(super) encoding: Encoding,
    /// 发起比较时文档是否有未保存的修改。
    pub(super) dirty: bool,
    /// 文档的语言，左右两侧都按该语言着色。
    pub(super) language: &'static winc_syntax::Language,
}

/// 已打开的 Git 差异标签。
pub(super) struct DiffTab {
    /// 比较的文件。
    pub(super) path: PathBuf,
    /// 左侧内容对应的 `HEAD` 提交号；`HEAD` 变化时才重新读取左侧内容。
    pub(super) oid: Option<String>,
}

/// 读取分支和改动数的位置数上限（个）。
///
/// 每个位置运行一次 `git status`；该上限避免包含大量仓库的目录在后台占用过多进程。
pub(super) const SUMMARY_REPOSITORIES: usize = 32;

/// 后台深层查找的结果：（查找的目录，找到的仓库）。
pub(super) type ScannedRepositories = (PathBuf, Vec<PathBuf>);
/// 等着确认的推送：要做的推送、会送出的提交的标题（最新的在前）和总数。
pub type PushPreview = (commit::PushIntent, Vec<String>, usize);

/// Git 模块的状态。
pub(crate) struct GitState {
    /// 当前仓库的根目录；模块关闭或目录中没有仓库时为 `None`。
    pub root: Option<PathBuf>,
    /// 打开的目录中可用的仓库（见 [`crate::app::App::git_discover`]）；有多个时由位置选择器列出。
    pub repositories: Vec<PathBuf>,
    /// 上次跟随的标签（[`crate::app::App::git_follow_active`]）；只在切换标签时查找文件所在的仓库。
    pub(super) followed: Option<DocumentId>,
    /// 各仓库最近一次读取的状态；位置选择器据此显示分支和改动数，切换仓库时先显示该状态。
    pub(super) summaries: Arc<Mutex<HashMap<PathBuf, Status>>>,
    /// 其他仓库（当前仓库之外）各文件的 Git 状态：多仓库文件夹里文件树的状态着色也覆盖它们，
    /// 摘要（[`crate::app::App::git_read_summaries`]）每次刷新时重建。
    pub(super) other_decorations: Arc<Mutex<HashMap<PathBuf, Change>>>,
    /// `summaries` 每改一次加 1（后台读到新状态、切换仓库时放进、取出）：按它缓存由这些状态
    /// 排出来的东西（见 `App::commit_other_rows`）。
    pub(super) summaries_version: Arc<std::sync::atomic::AtomicU64>,
    /// 各仓库最近一次读取的工作树；位置选择器据此在仓库下列出工作树。
    pub(super) summary_worktrees: Arc<Mutex<HashMap<PathBuf, Vec<Worktree>>>>,
    /// 离开各仓库时记下的分支、储藏、工作树和上一次提交，切换回去时先显示。
    pub(super) kept: HashMap<PathBuf, location_picker::KeptRefs>,
    /// 排好的全部位置，见 `App::cached_places`。
    pub(super) places: std::cell::RefCell<Option<location_picker::PlacesCache>>,
    /// 当前仓库各工作树相对当前 `HEAD` 的差异，与 `summaries` 一起读取。
    pub(super) summary_divergence: Arc<Mutex<HashMap<PathBuf, location_picker::Divergence>>>,
    /// 正在运行的读取；重新读取时取消。
    pub(super) summaries_running: Option<Cancel>,
    /// 后台深层查找（[`crate::app::App::git_scan_repositories`]）的结果。
    pub(super) scanned: Arc<Mutex<Option<ScannedRepositories>>>,
    /// 正在运行的深层查找；重新查找时取消。
    pub(super) scan_running: Option<Cancel>,
    /// 共用的 `git`（[`SharedGit`]），第一次使用时在后台查找。
    pub(in crate::app) tool: SharedGit,
    /// 最近一次成功的 `git status`。
    pub status: Option<Status>,
    /// 本地和远程分支（本地分支在前），与状态一起刷新。
    pub branches: Vec<Branch>,
    /// 当前仓库的各个标签，最近创建的在前；分支选择器的「标签」一组列出它们。
    pub tags: Vec<String>,
    /// 仓库配置的远程名，与状态一起刷新。
    pub remotes: Vec<String>,
    /// 仓库配置的远程仓库和地址，与状态一起刷新。
    pub remote_list: Vec<winc_editor::git::Remote>,
    /// 发布当前分支时使用的远程，与状态一起刷新；没有远程时为 `None`。
    pub publish_remote: Option<String>,
    /// 储藏，最新的在前，与状态一起刷新。
    pub stashes: Vec<Stash>,
    /// 最近切换到过的分支名，最近的在前，与状态一起刷新。
    pub recent_branches: Vec<String>,
    /// 仓库的工作树，主工作树在最前，与状态一起刷新；只有一个工作树时为空。
    pub worktrees: Vec<Worktree>,
    /// 仓库所在系统中用户的主目录，与分支一起刷新；`git.worktreeRoot` 开头的 `~/` 展开为该目录。
    pub home: Option<PathBuf>,
    /// 最近一次刷新失败的原因（找不到 `git`、`git` 报错）；刷新成功后清除。
    pub problem: Option<String>,
    /// `problem` 是否为没有可用的 `git`：这时提交视图不写「点击刷新按钮重试」。
    pub missing: bool,
    /// 状态的版本号，每更新一次加 1；提交视图据此判断绘制时记录的行是否仍然有效。
    pub generation: u64,
    /// 文件树使用的各路径状态（[`Status::decorations`]）。
    pub decorations: HashMap<PathBuf, Change>,
    /// 被忽略且未跟踪的文件和目录（[`Status::ignored`]）。
    ///
    /// 文件树以淡色绘制这些路径，其下的变化不触发状态刷新。
    pub(super) ignored_paths: HashSet<PathBuf>,
    /// 正在运行的刷新；`None` 表示没有在运行。
    pub(super) running: Option<Cancel>,
    /// 运行期间是否又收到刷新请求；为真时完成后再刷新一次。
    pub(super) again: bool,
    /// 下一次刷新是否同时读取分支与储藏（由 [`crate::app::App::git_refresh`] 设置，读取时清除）。
    pub(super) refs_wanted: bool,
    /// 刚完成的操作可能改动了引用（标签、远程分支）：下一次读到状态时重新读取已打开的日志。
    ///
    /// 状态中只能看出 `HEAD` 和分支的变化，标签的增删看不出。
    pub(super) logs_outdated: bool,
    /// 已打开的 Git 差异标签；状态刷新后按需原地更新，标签关闭时移除。
    pub(super) diff_tabs: HashMap<DocumentId, DiffTab>,
    /// 最近一次切换文件时的（差异标签，切换到的文件）。
    pub(super) stepped: Option<(DocumentId, PathBuf)>,
    /// 最近一次预览请求的序号。
    pub(in crate::app) preview_seq: u64,
    /// 本次运行中已询问过是否信任的仓库配置指纹（[`winc_editor::git::Risks::fingerprint`]）。
    ///
    /// 用户选择不信任后本次运行不再询问，下次启动时重新询问。
    pub(super) asked_trust: Vec<u64>,
    pub(super) sender: Sender<Outcome>,
    pub(super) receiver: Receiver<Outcome>,
    pub(super) prepared: (Sender<Prepared>, Receiver<Prepared>),
    /// 提交视图：勾选、提交信息。
    pub commit: commit::CommitState,
    /// 行号栏的改动标记和已展开的改动卡片。
    pub changes: changes::LineChanges,
    /// 提交视图上下文一行中各控件宽度的过渡。
    pub context_row: commit::ContextRow,
    /// 日志标签的后台通道和点击区域。
    pub log: log::LogState,
    /// 提交视图底部的提交图。
    pub graph: graph::GraphPart,
    /// 逐行追溯（`git blame`）的状态。
    pub blame: blame::BlameState,
    /// 正在进行的分支操作。
    pub branch: branch::BranchState,
    /// 文件内合并冲突的解决状态：识别出的冲突与可点击区域。
    pub merge: merge::MergeState,
    /// 位置选择器是否列出全部仓库和它们的工作树；否则只列当前仓库的工作树。
    pub location_all: bool,
    /// 后台读到的、等着确认的推送：要做的推送、会送出的提交的标题和总数（见
    /// [`crate::app::App::ask_push`]）。
    pub push_preview: Arc<Mutex<Option<PushPreview>>>,
    /// 推送对话框改了目标后在后台读到的提交（见 [`crate::app::App::refresh_push_list`]）。
    pub(in crate::app) push_outgoing: Arc<Mutex<Option<form::PushOutgoing>>>,
    /// 已提示过 `git` 版本过低：每次运行只提示一次。
    pub(in crate::app) old_git_warned: bool,
    /// 定时获取的状态（见 [`fetch`]）。
    pub(in crate::app) auto_fetch: fetch::AutoFetch,
    /// 当前仓库最近提交的作者（`名字 <邮箱>`），提交按钮的菜单里用来添加共同作者。
    pub(crate) authors: Vec<String>,
    /// 当前仓库里自己最近写的提交信息，最近的在前：在别处（终端、其他工具）提交的也在其中，
    /// 补在本程序记住的提交信息后面。
    pub(crate) own_messages: Vec<String>,
    /// 仓库配置的当前用户的邮箱（见 [`winc_editor::git::RecentHistory::me`]）；日志的作者筛选里
    /// 「我」按它筛选。没有读到时为空。
    pub(crate) my_email: String,
    /// 读作者时的仓库和它的 `HEAD`：换了仓库或 `HEAD` 动过（提交、拉取、切换）时重读。
    pub(super) authors_for: Option<(PathBuf, Option<String>)>,
    /// 已经读过 `git` 版本的仓库：每个仓库每次运行只读一次，不随每次刷新多运行一条命令。
    pub(super) version_read: HashSet<PathBuf>,
    /// 开着的 Git 输出标签已经显示到记录的第几个字节（见 `winc_editor::git::output_snapshot`）；
    /// 标签里是「暂无输出」的说明时为 0。
    pub output_shown: u64,
    /// 分支选择器里改变过默认展开状态的目录：（是否远程，目录路径）。
    pub branch_folders: HashSet<(bool, String)>,
}

impl Default for GitState {
    fn default() -> Self {
        let (sender, receiver) = channel();
        GitState {
            root: None,
            repositories: Vec::new(),
            followed: None,
            summaries: Arc::default(),
            other_decorations: Arc::default(),
            summaries_version: Arc::default(),
            summary_worktrees: Arc::default(),
            kept: HashMap::new(),
            places: std::cell::RefCell::new(None),
            summary_divergence: Arc::default(),
            summaries_running: None,
            scanned: Arc::default(),
            scan_running: None,
            tool: SharedGit::default(),
            status: None,
            branches: Vec::new(),
            tags: Vec::new(),
            remotes: Vec::new(),
            remote_list: Vec::new(),
            publish_remote: None,
            stashes: Vec::new(),
            recent_branches: Vec::new(),
            worktrees: Vec::new(),
            home: None,
            problem: None,
            missing: false,
            generation: 0,
            decorations: HashMap::new(),
            ignored_paths: HashSet::new(),
            running: None,
            again: false,
            refs_wanted: false,
            logs_outdated: false,
            diff_tabs: HashMap::new(),
            stepped: None,
            preview_seq: 0,
            asked_trust: Vec::new(),
            sender,
            receiver,
            commit: Default::default(),
            changes: Default::default(),
            context_row: Default::default(),
            log: Default::default(),
            graph: Default::default(),
            blame: Default::default(),
            branch: Default::default(),
            merge: Default::default(),
            location_all: false,
            output_shown: 0,
            push_preview: Arc::default(),
            push_outgoing: Arc::default(),
            old_git_warned: false,
            version_read: HashSet::new(),
            authors: Vec::new(),
            own_messages: Vec::new(),
            my_email: String::new(),
            authors_for: None,
            auto_fetch: fetch::AutoFetch::default(),
            branch_folders: HashSet::new(),
            prepared: channel(),
        }
    }
}

impl GitState {
    /// 记下各仓库的状态（`summaries`）改过了。
    pub(crate) fn summaries_changed(&self) {
        (self.summaries_version).fetch_add(1, std::sync::atomic::Ordering::Release);
    }

    /// 各仓库状态的版本，见 `summaries_version`。
    pub(crate) fn summaries_version(&self) -> u64 {
        self.summaries_version
            .load(std::sync::atomic::Ordering::Acquire)
    }

    /// 返回文件树中 `path` 显示的状态。
    ///
    /// 当前仓库用实时的状态（[`Self::decorations`]）；文件夹里其他仓库的文件用各自摘要状态
    /// 换算的装饰（见 [`Self::other_decorations`]），切了分支、提交之后随摘要刷新。
    pub fn change_of(&self, path: &Path) -> Option<Change> {
        if let Some(change) = self.decorations.get(path) {
            return Some(*change);
        }
        // 当前仓库内的路径：装饰里没有的就是没有改动，不回退（摘要可能是过时的）。
        if self
            .root
            .as_deref()
            .is_some_and(|root| path.starts_with(root))
        {
            return None;
        }
        self.other_decorations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(path)
            .copied()
    }

    /// `path` 是否被 Git 忽略：`path` 本身或任一上级目录位于最近一次状态的忽略列表中。
    pub fn is_ignored(&self, path: &Path) -> bool {
        if self.ignored_paths.is_empty() {
            return false;
        }
        let root = self.root.as_deref();
        path.ancestors()
            .take_while(|p| Some(*p) != root)
            .any(|p| self.ignored_paths.contains(p))
    }

    /// 切换到仓库 `next` 之前清空状态。
    ///
    /// `next` 是同一仓库的另一个工作树时保留共用的工作树列表和远程，使徽章、位置行和同步控件
    /// 在后台读取完成之前即显示正确内容，不出现短暂的空白。
    pub(in crate::app) fn clear_for(&mut self, next: Option<&Path>) {
        let same_repository = next.is_some_and(|next| self.shares_repository(next));
        let worktrees = std::mem::take(&mut self.worktrees);
        let remotes = std::mem::take(&mut self.remotes);
        self.clear();
        if same_repository {
            self.worktrees = worktrees;
            self.remotes = remotes;
        }
    }

    /// 停止正在运行的刷新并清空状态。
    ///
    /// 未找到 `git` 的结论同样清除，下次重新查找。
    pub(super) fn clear(&mut self) {
        if let Some(cancel) = self.running.take() {
            cancel.cancel();
        }
        self.again = false;
        self.refs_wanted = false;
        self.root = None;
        self.tool.forget();
        self.status = None;
        self.branches.clear();
        self.tags.clear();
        self.remotes.clear();
        self.remote_list.clear();
        self.publish_remote = None;
        self.stashes.clear();
        self.worktrees.clear();
        self.home = None;
        self.authors.clear();
        self.own_messages.clear();
        self.my_email.clear();
        self.authors_for = None;
        self.problem = None;
        self.missing = false;
        self.generation += 1;
        self.decorations.clear();
        self.ignored_paths.clear();
    }
}
