//! Git 模块共用的数据结构与状态类型：提交、分支、引用、变更条目、仓库状态与操作类型。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// `HEAD` 指向的提交（提交视图里「撤销上一次提交」使用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastCommit {
    /// 完整的提交号。
    pub oid: String,
    /// 提交信息的第一行。
    pub subject: String,
    /// 完整的提交信息。
    pub message: String,
    /// 提交时间（Unix 秒）。
    pub time: i64,
    /// 提交时间在本机时区的写法「2026-10-26 14:32」。
    pub local: String,
}

/// 提交时的选项，见 [`super::Git::commit_with`]。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommitOptions {
    /// 把全部已跟踪文件的改动一并提交（`git commit --all`）；提交失败时暂存区保持原状。
    pub all: bool,
    /// 修改上一次提交；暂存区没有改动时只改提交信息。
    pub amend: bool,
    /// 在提交信息末尾附加提交者的签署行（`--signoff`）。
    pub sign_off: bool,
    /// 不运行 `pre-commit` 和 `commit-msg` 钩子（`--no-verify`）。
    pub no_verify: bool,
}

/// 一个分支。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// 分支名；远程分支带着远程名（`origin/main`）。
    pub name: String,
    /// 是否是远程分支。
    pub remote: bool,
    /// 是否是当前分支。
    pub current: bool,
    /// 跟踪的上游分支（`origin/main`）；没有，或者上游引用已不存在时为 `None`。
    pub upstream: Option<String>,
    /// 配了上游，但上游引用已不存在（远程分支被删除后获取时清掉了）。
    ///
    /// 这样的分支推送过，不能和从未推送过的分支一样看待。
    pub gone: bool,
    /// 该分支指向的完整提交号。
    ///
    /// 提交、推送、获取使分支移动时随之变化，据此判断开着的日志是否需要重新读取。
    pub oid: String,
    /// 分支指向的提交的提交时间（Unix 秒）。
    pub time: i64,
    /// 提交时间在本机时区的写法「2026-10-26 14:32」。
    pub local: String,
    /// 领先上游的提交数；没有上游时为 0，超过 `u16::MAX` 时记作 `u16::MAX`。
    pub ahead: u16,
    /// 落后上游的提交数；没有上游时为 0，超过 `u16::MAX` 时记作 `u16::MAX`。
    pub behind: u16,
}

/// 日志里的一个提交。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// 完整的提交号。
    pub oid: String,
    /// 父提交的提交号，第一个是主线；根提交没有，合并提交有两个以上。
    pub parents: Vec<String>,
    /// 作者的名字。
    pub author: String,
    /// 作者的邮箱。
    pub email: String,
    /// 作者写下这次提交的时间（Unix 秒）。
    pub time: i64,
    /// 同一时间换成本机时区的写法：「2026-10-26 14:32」。
    pub local: String,
    /// 指向该提交的引用：分支、远程分支、标签、`HEAD`。
    pub refs: Vec<Ref>,
    /// 说明的第一行。
    pub subject: String,
}

/// 指着一个提交的引用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ref {
    /// `HEAD` 指着的当前分支。
    Head(String),
    /// `HEAD` 直接指向该提交（分离的 `HEAD`）。
    Detached,
    /// 本地分支。
    Branch(String),
    /// 远程分支（带远程名，`origin/main`）。
    Remote(String),
    /// 标签。
    Tag(String),
}

/// 一次提交改动的一个文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitFile {
    /// 文件的绝对路径（改之后的路径）。
    pub path: PathBuf,
    /// 改名前的绝对路径（只有改名时才有）。
    pub from: Option<PathBuf>,
    /// 怎么变的：新增、修改、删除、改名。
    pub change: Change,
    /// 加了几行、删了几行；二进制文件为 `None`。
    pub lines: Option<(usize, usize)>,
}

/// 一条储藏（`git stash`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    /// 名字：`stash@{0}`，最新的是 0。
    ///
    /// 新建或删除储藏后，其余储藏的编号随之改变；要指定某一条储藏时用 `oid`。
    pub name: String,
    /// 储藏对应的提交号（完整的对象号），不随其他储藏的增删改变。
    pub oid: String,
    /// 储藏提交的各个父提交：第一个是储藏时的 `HEAD`，第二个是暂存区，连同未跟踪的文件一起储藏时
    /// 第三个是那些文件。
    pub parents: Vec<String>,
    /// 储藏的时间（Unix 秒）。
    pub time: i64,
    /// 储藏时的本地时间，「2026-11-07 14:32」。
    pub local: String,
    /// `git` 记下的说明：「On main: 说明」或「WIP on main: 提交号 说明」。
    pub message: String,
}

impl Stash {
    /// 储藏时所在的分支：说明开头「On 分支:」「WIP on 分支:」里的分支；分离 `HEAD` 时 `git` 写
    /// 「(no branch)」，原样返回。说明不是这两种写法时为 `None`。
    pub fn branch(&self) -> Option<&str> {
        let rest =
            (self.message.strip_prefix("WIP on ")).or_else(|| self.message.strip_prefix("On "))?;
        rest.split_once(": ").map(|(branch, _)| branch)
    }
}

/// 逐行追溯（`git blame`）的结果：每一行最后一次由哪次提交改动。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Blame {
    /// 出现过的提交，按第一次出现的顺序。
    pub commits: Vec<BlameCommit>,
    /// 每一行（从 0 起）是 `commits` 里的第几个。
    pub lines: Vec<u32>,
}

/// 逐行追溯里的一次提交。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlameCommit {
    /// 完整的提交号；尚未提交的行全是 0。
    pub oid: String,
    /// 作者的名字。
    pub author: String,
    /// 作者写下这次提交的时间（Unix 秒）。
    pub time: i64,
    /// 作者所在时区与 UTC 的差（秒），东八区是 28800。
    pub offset: i64,
    /// 说明的第一行。
    pub summary: String,
    /// 这些行还没有提交（在当前工作区里改过、尚未提交的行）。
    pub uncommitted: bool,
}

/// 日志的筛选条件。
///
/// 都不填时是所有分支的全部提交。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogQuery {
    /// 说明里含有这段文字（不分大小写，按字面找，不是正则）。
    pub text: String,
    /// 只看这些分支（本地或远程，如 `origin/main`）能走到的提交；空着是全部分支。
    pub branches: Vec<String>,
    /// 作者名字或邮箱里含有其中任一段文字（不分大小写，按字面找）；空着是全部作者。
    pub authors: Vec<String>,
    /// 只看改过该文件的提交（文件历史），越过改名继续往前查找。
    pub path: Option<PathBuf>,
    /// 与 `path` 一起使用：只看改过这几行的提交（`git log -L`），按 `HEAD` 中的行号，从 1 起，
    /// 首尾都含。`git` 随各次提交挪动这段范围，越过改名继续往前查找。
    pub lines: Option<(usize, usize)>,
    /// 固定在最前面的提交（完整的提交号），见 [`super::Git::log_stream`]；「转到提交号、分支或标签」
    /// 找到的提交不在已列出的提交中时使用。
    pub pin: Option<String>,
    /// 只看最近几天的提交（`--since`）；日期菜单里常用的几项用它。
    pub since_days: Option<u32>,
    /// 只看这个时间之后的提交，按 `git log --since` 认识的样子写（如 `2024-01-01`、`3 weeks
    /// ago`）；日期菜单里「其他时间…」填的。与 [`Self::since_days`] 只用一个，两个都填时按天数。
    pub since: Option<String>,
    /// 只看改过这些路径的提交：相对仓库根目录的路径或通配（`git` 的路径规格），目录包括其中
    /// 的全部文件。与文件历史的 `path` 不同时使用。
    pub paths: Vec<String>,
}

impl LogQuery {
    /// 是否因为按说明、作者或文件筛选而画不出提交图。
    ///
    /// 这些条件下结果里的父提交大多不在列表里，提交图连不起来；只按分支筛选时提交仍然相连，
    /// 可以画图；按路径筛选时父提交按路径改写（[`Self::rewrites_parents`]），同样可以画图。
    pub fn graphless(&self) -> bool {
        !self.text.trim().is_empty()
            || !self.authors.is_empty()
            || self.path.is_some()
            || self.pin.is_some()
    }

    /// 是否按路径筛选、父提交按路径改写（`--parents`）：每个提交的父提交是它往下最近一个也改过
    /// 这些路径的提交，提交图照样连得起来。真正的父提交另外查（见 [`super::log::Details::base`]）。
    pub fn rewrites_parents(&self) -> bool {
        !self.paths.is_empty() && self.path.is_none()
    }

    /// 是否按说明、分支或作者筛选过，或固定了一个提交。
    ///
    /// 文件历史的文件不算：那本来就是该标签展示的内容。
    pub fn filtered(&self) -> bool {
        !self.text.trim().is_empty()
            || !self.branches.is_empty()
            || !self.authors.is_empty()
            || self.pin.is_some()
            || self.since_days.is_some()
            || self.since.is_some()
            || !self.paths.is_empty()
    }
}

/// 一个文件相对于某个版本的变化。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Change {
    /// 被删除。
    Deleted,
    /// 未跟踪的新文件。
    Untracked,
    /// 新加入版本管理。
    Added,
    /// 被改名（可能同时被修改）。
    Renamed,
    /// 内容或类型被修改。
    Modified,
    /// 合并时有冲突。
    Conflicted,
}

impl Change {
    /// 状态的单字母写法，用在文件树和变更列表里。
    pub fn letter(self) -> char {
        match self {
            Change::Deleted => 'D',
            Change::Untracked => 'U',
            Change::Added => 'A',
            Change::Renamed => 'R',
            Change::Modified => 'M',
            Change::Conflicted => '!',
        }
    }

    /// 把 `git status --porcelain=v2` 中 `XY` 的一位字母转换成变化类型，`.` 表示没有变化。
    pub(super) fn from_code(code: u8) -> Option<Change> {
        match code {
            b'M' | b'T' => Some(Change::Modified),
            b'A' | b'C' => Some(Change::Added),
            b'D' => Some(Change::Deleted),
            b'R' => Some(Change::Renamed),
            b'U' => Some(Change::Conflicted),
            _ => None,
        }
    }
}

/// 状态里的一个文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// 文件的绝对路径。
    pub path: PathBuf,
    /// 改名前的绝对路径（只有改名时才有）。
    pub from: Option<PathBuf>,
    /// 已暂存的变化：暂存区相对于 `HEAD`。
    pub staged: Option<Change>,
    /// 未暂存的变化：工作区相对于暂存区。
    ///
    /// 未跟踪、冲突的文件只有这一项。
    pub unstaged: Option<Change>,
    /// 该条目是整个未被跟踪的目录（`git` 只报一项，路径以 `/` 结尾）；读状态时记下，绘制时
    /// 不再查询文件系统。
    pub is_dir: bool,
}

impl Entry {
    /// 是否是未跟踪的文件（或目录）。
    pub fn untracked(&self) -> bool {
        self.unstaged == Some(Change::Untracked)
    }

    /// 是否为合并时有冲突的文件。
    pub fn conflicted(&self) -> bool {
        self.staged == Some(Change::Conflicted) || self.unstaged == Some(Change::Conflicted)
    }

    /// 在文件树里显示的状态：两项中优先级更高的一项。
    pub fn shown(&self) -> Change {
        match (self.staged, self.unstaged) {
            (Some(a), Some(b)) => a.max(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => Change::Modified,
        }
    }
}

/// `HEAD` 指向哪里。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    /// 在某个分支上。
    Branch(String),
    /// 分离的 `HEAD`，值是提交号的前 7 位。
    Detached(String),
}

/// `git status` 最多读取的有变化的文件数（个）。
///
/// 超过时只保留前 `STATUS_LIMIT` 个，并标记 [`Status::truncated`]。
///
/// 没被忽略的数据目录、虚拟环境可以有数十万个文件，每次刷新都重建全部条目会占用数百 MB 内存。
pub const STATUS_LIMIT: usize = 10_000;

/// 一次 `git status` 的结果。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    /// 当前的 `HEAD`；仓库状态异常、读不出时为 `None`。
    pub head: Option<Head>,
    /// `HEAD` 指向的完整提交号；还没有提交过时为 `None`。
    ///
    /// `HEAD` 变化时（提交、切换分支），提交号随之变化。
    /// 据此判断各文件在 `HEAD` 里的内容是否需要重新取。
    pub oid: Option<String>,
    /// 有上游分支时，本地领先的提交数。
    pub ahead: u32,
    /// 有上游分支时，本地落后的提交数。
    pub behind: u32,
    /// 有变化的文件，按 `git` 输出的顺序（路径排序）。
    pub entries: Vec<Entry>,
    /// 被忽略规则（各级 `.gitignore`、`.git/info/exclude`、全局忽略文件）忽略且未跟踪的文件和目录。
    ///
    /// 整个目录被忽略时只包含目录本身，不列出其中的文件（`--ignored=matching`）。
    pub ignored: Vec<PathBuf>,
    /// 正在进行、停下来等用户处理冲突的操作（合并、变基……）；没有时为 `None`。
    pub operation: Option<Operation>,
    /// 进行中的变基的进度：（正在处理第几个提交，共几个）；不在变基中、无法读取时为 `None`。
    pub progress: Option<(u32, u32)>,
    /// 有变化的文件超过 [`STATUS_LIMIT`] 个、`entries` 只含前一部分时为 `true`。
    pub truncated: bool,
    /// 仍含冲突标记（`<<<<<<<` … `>>>>>>>`）的冲突文件。
    ///
    /// 提交这些文件会将冲突标记一并提交。读取状态时在后台同时读取这些文件。
    pub unresolved: Vec<PathBuf>,
    /// 仓库的 `.git` 目录；工作树和子模块里是 `.git` 文件所指的目录。读不到时为 `None`。
    pub git_dir: Option<PathBuf>,
}

/// 停下来等用户处理冲突的操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    /// `git merge`（还有 `pull` 时的合并）。
    Merge,
    /// `git rebase`。
    Rebase,
    /// `git cherry-pick`。
    CherryPick,
    /// `git revert`。
    Revert,
}

impl Operation {
    /// 命令的名字，例如「合并」「变基」。
    pub fn label(self) -> &'static str {
        match self {
            Operation::Merge => winc_i18n::t!("git-operation-merge"),
            Operation::Rebase => winc_i18n::t!("git-operation-rebase"),
            Operation::CherryPick => winc_i18n::t!("git-operation-cherry-pick"),
            Operation::Revert => winc_i18n::t!("git-operation-revert"),
        }
    }

    pub(super) fn command(self) -> &'static str {
        match self {
            Operation::Merge => "merge",
            Operation::Rebase => "rebase",
            Operation::CherryPick => "cherry-pick",
            Operation::Revert => "revert",
        }
    }

    /// 从仓库目录 `git_dir` 里的标记文件看出正在进行的操作。
    pub(super) fn detect(git_dir: &Path) -> Option<Operation> {
        // 各标记的文件信息一次取回：远端的仓库只需一次往返。先列出的优先。
        let markers = [
            ("rebase-merge", true, Operation::Rebase),
            ("rebase-apply", true, Operation::Rebase),
            ("MERGE_HEAD", false, Operation::Merge),
            ("CHERRY_PICK_HEAD", false, Operation::CherryPick),
            ("REVERT_HEAD", false, Operation::Revert),
        ];
        let paths = markers.map(|(name, _, _)| git_dir.join(name));
        let found = winc_backend::fs::metadata_many(&paths);
        (markers.into_iter().zip(found)).find_map(|((_, dir, operation), metadata)| {
            let metadata = metadata.ok()?;
            let present = if dir {
                metadata.is_dir()
            } else {
                metadata.is_file()
            };
            present.then_some(operation)
        })
    }
}

/// 返回仓库目录 `git_dir` 中进行中的变基的进度：（正在处理第几个提交，共几个）。
///
/// 两种变基方式各用一个目录记录，文件名不同：`rebase-merge` 中是 `msgnum` 和 `end`，
/// `rebase-apply` 中是 `next` 和 `last`。
pub(super) fn rebase_progress(git_dir: &Path) -> Option<(u32, u32)> {
    let number = |path: PathBuf| -> Option<u32> {
        let text = winc_backend::fs::read_to_string(path).ok()?;
        text.trim().parse().ok()
    };
    let layouts = [
        ("rebase-merge", "msgnum", "end"),
        ("rebase-apply", "next", "last"),
    ];
    layouts.into_iter().find_map(|(dir, at, total)| {
        let dir = git_dir.join(dir);
        Some((number(dir.join(at))?, number(dir.join(total))?))
    })
}

/// 仓库 `root` 的 `.git` 目录：工作树和子模块里 `.git` 是一个写着 `gitdir: <路径>` 的文件。
pub(super) fn git_dir(root: &Path) -> Option<PathBuf> {
    let dot = root.join(".git");
    if winc_backend::fs::is_dir(&dot) {
        return Some(dot);
    }
    let text = winc_backend::fs::read_to_string(&dot).ok()?;
    let target = text.trim().strip_prefix("gitdir:")?.trim();
    Some(crate::host::resolve(root, target))
}

/// 仓库共用的 `git` 目录：工作树（`git worktree`）的 `.git` 文件指向它自己的目录，hooks、
/// 各工作树的记录（`worktrees/`）在那里的 `commondir` 所指的目录里。
pub(super) fn git_common_dir(root: &Path) -> Option<PathBuf> {
    let dir = git_dir(root)?;
    match winc_backend::fs::read_to_string(dir.join("commondir")) {
        Ok(common) => Some(dir.join(common.trim())),
        Err(_) => Some(dir),
    }
}

impl Status {
    /// 文件树要用的状态：每个有变化的文件，以及它们所在的各级目录（直到 `root`，不含 `root`）。
    ///
    /// 目录取子项里最显眼的状态，但删除不传给目录：目录里删掉一个文件，目录本身不显示异常。
    pub fn decorations(&self, root: &Path) -> HashMap<PathBuf, Change> {
        let mut map: HashMap<PathBuf, Change> = HashMap::new();
        for entry in &self.entries {
            let shown = entry.shown();
            map.insert(entry.path.clone(), shown);
            if shown == Change::Deleted {
                continue;
            }
            for dir in entry.path.ancestors().skip(1) {
                if dir == root || !dir.starts_with(root) {
                    break;
                }
                match map.entry(dir.to_path_buf()) {
                    // 已经有同样显眼或更显眼的状态：再往上的各级目录也已经有了。
                    std::collections::hash_map::Entry::Occupied(slot) if *slot.get() >= shown => {
                        break;
                    }
                    std::collections::hash_map::Entry::Occupied(mut slot) => {
                        slot.insert(shown);
                    }
                    std::collections::hash_map::Entry::Vacant(slot) => {
                        slot.insert(shown);
                    }
                }
            }
        }
        map
    }
}

/// 找不到可用的 `git` 的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unavailable {
    /// `PATH` 里没有 `git`。
    NotInstalled,
    /// 设置的程序（`git.path`）不存在或不能运行，内容是设置的写法。
    NotFound(String),
    /// macOS 上没有安装命令行开发者工具：`/usr/bin/git` 是占位程序，运行后只会弹出系统的
    /// 安装对话框。
    NoDeveloperTools,
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unavailable::NotInstalled => {
                f.write_str(winc_i18n::t!("git-unavailable-not-installed"))
            }
            Unavailable::NotFound(program) => {
                let program = program.as_str();
                f.write_str(&winc_i18n::t!(
                    "git-unavailable-not-found",
                    program = program
                ))
            }
            Unavailable::NoDeveloperTools => {
                f.write_str(winc_i18n::t!("git-unavailable-no-developer-tools"))
            }
        }
    }
}

/// [`super::Git::delete_branch`] 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchDeleted {
    /// 已删除，内容是 `git` 的结论。
    Done(String),
    /// 分支未完全合并，需要强制删除，尚未删除。
    NeedsForce,
}
