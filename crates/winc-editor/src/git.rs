//! Git 模块的底层：找到系统里的 `git`，运行 `git status` 并解析结果，取文件在暂存区或提交里的内容。
//!
//! 只调用 `git` 命令，不链接 Git 的库，用户的 hooks、全局配置、凭据助手都仍然生效；界面线程不调用这
//! 里的函数。仓库自己的配置（`.git/config`、hooks）可能来自不可信的压缩包或共享盘，可以指定 `git`
//! 要运行的命令：用户信任该仓库（[`Git::trust`]）之前不运行 fsmonitor 和过滤器，会改动仓库的操作一
//! 律拒绝，依据见 [`Risks`]。

pub mod log;
pub mod merge;

mod commit;
mod discover;
mod output;
mod parse;
mod rebase;
mod remote;
mod sync;
mod types;

pub use commit::RecentHistory;
pub use discover::{find_repositories, find_root, is_commit_hash, scan_repositories};
pub use output::{on_output, output_since, output_snapshot, output_text};
pub use rebase::{RebaseAction, RebaseCommit, RebasePlan, RebaseStep, rebase_todo};
pub use remote::{Remote, TagTarget, repository_dir_name};
pub use sync::{COMPARE_LIMIT, Carried, Divergence, MINIMUM_VERSION, Outgoing, Synced};
pub use types::{
    Blame, BlameCommit, Branch, BranchDeleted, Change, Commit, CommitFile, CommitOptions, Entry,
    Head, LastCommit, LogQuery, Operation, Ref, STATUS_LIMIT, Stash, Status, Unavailable,
};
mod history;
mod locate;
pub use locate::runs_here;
mod trust;
mod worktree;

use parse::*;
pub use trust::*;
use types::{git_common_dir, git_dir, rebase_progress};
pub use worktree::*;

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use winc_core::hash::Fnv;

use crate::tools::shell::Shell;
use crate::tools::shell::process::{self, Cancel, RunError};

/// `git status` 最长的运行时间（30 秒）。
///
/// 几十万个文件的仓库第一次运行需要数秒；超过该时间视为 `git` 失去响应。
pub(crate) const STATUS_TIMEOUT: Duration = Duration::from_secs(30);
/// 日志中日期的格式参数。
///
/// 本机时区的换算由 `git` 完成（`format-local`），不需要读取时区数据库。
const LOG_DATE: &str = "--date=format-local:%Y-%m-%d %H:%M";
/// 日志中单个提交的输出格式，由 [`parse_log`] 解析。
const LOG_FORMAT: &str = "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%ad%x1f%D%x1f%s%x1e";
/// 取文件内容最长的运行时间（10 秒）。
///
/// 读取大仓库里的对象通常在毫秒级；超过该时间视为 `git` 失去响应。
const SHOW_TIMEOUT: Duration = Duration::from_secs(10);
/// 提交最长的运行时间（10 分钟）：提交前的 hooks 可能要运行测试、格式检查。
const COMMIT_TIMEOUT: Duration = Duration::from_secs(600);
/// 获取、拉取、推送、合并、变基这类操作最长的运行时间（10 分钟）。
///
/// 网络慢、hooks 需要运行测试或格式检查时耗时较长。
const OPERATION_TIMEOUT: Duration = Duration::from_secs(600);
/// 决定 `git` 作用于哪个仓库、暂存区和对象库的环境变量，运行 `git` 时不从环境继承。
///
/// 列表取自 `git rev-parse --local-env-vars`，其中的 `GIT_CONFIG_PARAMETERS`、`GIT_CONFIG_COUNT`
/// 除外：二者只携带配置项，`git` 进入子模块时同样保留。本程序由 `git` 启动时（作为编辑器或在钩子
/// 中运行）继承这些变量；例如 `git commit` 传给编辑器的 `GIT_INDEX_FILE` 指向一个临时暂存区，
/// 提交结束后该文件即被删除。
const REPOSITORY_ENV: [&str; 13] = [
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG",
    "GIT_OBJECT_DIRECTORY",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_GRAFT_FILE",
    "GIT_INDEX_FILE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_REPLACE_REF_BASE",
    "GIT_PREFIX",
    "GIT_SHALLOW_FILE",
    "GIT_COMMON_DIR",
];

/// 日志最长的运行时间（30 秒，经验值）。
///
/// 几十万个提交的仓库取一批也只要零点几秒。
const LOG_TIMEOUT: Duration = Duration::from_secs(30);

/// 检查 macOS 开发者工具是否已安装的最长时间（5 秒，经验值）。
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// 系统里可用的 `git`。
#[derive(Debug, Clone)]
pub struct Git {
    /// `git` 可执行文件的路径。
    program: PathBuf,
    /// 本机是否有可用的 `git`；为否时只有发行版中的仓库可用（发行版内的 `git`）。
    local: bool,
    /// 用户 shell 的环境：`git` 命令都在这个环境里运行。
    shell: Shell,
    /// 各仓库的配置检查结果和信任状态，[`Git`] 的各个副本共用。
    trust: Arc<Mutex<Trust>>,
    /// 处理输入请求的 askpass 程序，见 [`Git::with_askpass`]。
    askpass: Option<Askpass>,
}

/// askpass 程序作为凭据助手运行时的第一个参数，其后是 `git` 给出的动作。
pub const CREDENTIAL_HELPER_ARG: &str = "git-credential";

/// 传递 [`Askpass::endpoint`] 的环境变量名。
pub const ASKPASS_ENDPOINT: &str = "WINC_GIT_ASKPASS";

/// `git`、`ssh` 需要用户输入时运行的 askpass 程序。
#[derive(Debug, Clone)]
pub struct Askpass {
    /// 程序路径：程序以提示文字为参数启动，把答复写到标准输出。
    pub program: PathBuf,
    /// 程序连接的本地端点，由环境变量 [`ASKPASS_ENDPOINT`] 传递。
    pub endpoint: OsString,
    /// 是否同时把该程序配置为排在最后的凭据助手（[`CREDENTIAL_HELPER_ARG`]）。
    pub credential_helper: bool,
}

impl Git {
    /// 仓库 `root` 是否有可用的 `git`：本机的仓库需要本机的 `git`，发行版和远端主机上的仓库
    /// 使用各自系统中的 `git`。
    pub fn reaches(&self, root: &Path) -> bool {
        self.local || winc_backend::is_remote(root) || crate::wsl::locate_here(root).is_some()
    }

    /// 在仓库 `root` 里运行 `git status`。
    ///
    /// # Errors
    ///
    /// `git` 运行失败、超时、被取消或者输出读不懂时返回原因。
    pub fn status(&self, root: &Path, cancel: &Cancel) -> Result<Status, String> {
        if !self.reaches(root) {
            return Err(Unavailable::NotInstalled.to_string());
        }
        self.inspect(root)?;
        let mut command = self.command(root);
        command.args([
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
            // 文件树以淡色绘制被忽略的项。`matching` 只列出匹配忽略规则的那一层，不列出被忽略
            // 目录中的内容（`target/`、`node_modules/` 下可能有数万个文件）。
            "--ignored=matching",
            "--ignore-submodules=dirty",
        ]);
        // 正在进行的操作从 `.git` 里的标记文件看出，与 `git status` 互不依赖：远端的仓库上
        // 同时进行。
        let marks = || {
            let dir = git_dir(root);
            let operation = dir.as_deref().and_then(Operation::detect);
            (dir, operation)
        };
        let run = || self.run(&mut command, STATUS_TIMEOUT, cancel);
        let remote = winc_backend::is_remote(root);
        let ((dir, operation), stdout) = crate::background::both(remote, marks, run);
        let mut status = parse_status(&stdout?, root)
            .ok_or_else(|| winc_i18n::t!("git-status-unreadable").to_owned())?;
        if operation == Some(Operation::Rebase) {
            status.progress = dir.as_deref().and_then(rebase_progress);
        }
        (status.git_dir, status.operation) = (dir, operation);
        status.unresolved = status
            .entries
            .iter()
            .filter(|e| e.conflicted() && merge::file_has_conflicts(&e.path))
            .map(|e| e.path.clone())
            .collect();
        Ok(status)
    }

    /// 处理完冲突以后继续 `operation`（例如 `git rebase --continue`）。
    ///
    /// 合并的继续由提交视图提交完成，不经过该函数。
    ///
    /// # Errors
    ///
    /// 还有未解决的冲突、hooks 拒绝时返回 `git` 给出的原因。
    pub fn continue_operation(&self, root: &Path, operation: Operation) -> Result<String, String> {
        self.operate(root, &[operation.command(), "--continue"])
    }

    /// 跳过 `operation` 正在处理的提交，继续处理其后的提交（例如 `git rebase --skip`）。
    ///
    /// 该提交的更改和已经解决的冲突被丢弃。合并没有可以跳过的提交。
    ///
    /// # Errors
    ///
    /// `git` 报错时返回它的说明。
    pub fn skip_operation(&self, root: &Path, operation: Operation) -> Result<String, String> {
        self.operate(root, &[operation.command(), "--skip"])
    }

    /// 放弃 `operation`，回到开始之前的状态（例如 `git merge --abort`）。
    ///
    /// # Errors
    ///
    /// `git` 报错时返回它的说明。
    pub fn abort_operation(&self, root: &Path, operation: Operation) -> Result<String, String> {
        self.operate(root, &[operation.command(), "--abort"])
    }

    /// 合并的提交信息草稿（`.git/MERGE_MSG`，去掉 `#` 开头的说明行）；不在合并中时为 `None`。
    pub fn merge_message(&self, root: &Path) -> Option<String> {
        let text = winc_backend::fs::read_to_string(git_dir(root)?.join("MERGE_MSG")).ok()?;
        let kept: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
        Some(kept.join("\n").trim().to_owned())
    }

    /// 本地和远程分支，本地的在前，各自按名字排列。
    ///
    /// 远程的 `HEAD` 指针不列出。
    ///
    /// # Errors
    ///
    /// `git` 运行失败时返回原因。
    pub fn branches(&self, root: &Path, cancel: &Cancel) -> Result<Vec<Branch>, String> {
        let mut command = self.command(root);
        command.args([
            "for-each-ref",
            "--format=%(refname)%00%(HEAD)%00%(upstream:short)%00%(objectname)\
             %00%(committerdate:unix)%00%(committerdate:format-local:%Y-%m-%d %H:%M)\
             %00%(upstream:track,nobracket)",
            "refs/heads",
            "refs/remotes",
        ]);
        let stdout = self.run(&mut command, STATUS_TIMEOUT, cancel)?;
        Ok(parse_branches(&String::from_utf8_lossy(&stdout)))
    }

    /// 返回仓库中各个标签的名字，最近创建的在前。
    ///
    /// # Errors
    ///
    /// `git` 运行失败或超时时返回原因。
    pub fn tags(&self, root: &Path, cancel: &Cancel) -> Result<Vec<String>, String> {
        let mut command = self.command(root);
        // `lstrip=2` 去掉 `refs/tags/`：`short` 在标签与分支同名时会带上 `tags/`。
        let format = "--format=%(refname:lstrip=2)";
        command.args(["for-each-ref", "--sort=-creatordate", format, "refs/tags"]);
        let stdout = self.run(&mut command, STATUS_TIMEOUT, cancel)?;
        let names = String::from_utf8_lossy(&stdout);
        Ok(names.lines().map(str::to_owned).collect())
    }

    /// 返回仓库配置的远程名。
    ///
    /// # Errors
    ///
    /// `git` 运行失败或超时时返回原因。
    pub fn remotes(&self, root: &Path, cancel: &Cancel) -> Result<Vec<String>, String> {
        let mut command = self.command(root);
        command.arg("remote");
        let stdout = self.run(&mut command, STATUS_TIMEOUT, cancel)?;
        Ok(String::from_utf8_lossy(&stdout)
            .lines()
            .map(str::trim)
            .filter(|remote| !remote.is_empty())
            .map(str::to_owned)
            .collect())
    }

    /// 返回 `root` 所在仓库的公共 Git 目录：提交、分支都记在这里，附属工作树也用它。
    ///
    /// # Errors
    ///
    /// `git` 运行失败或超时时返回原因。
    pub fn common_dir(&self, root: &Path, cancel: &Cancel) -> Result<PathBuf, String> {
        let mut command = self.command(root);
        command.args(["rev-parse", "--git-common-dir"]);
        let stdout = self.run(&mut command, STATUS_TIMEOUT, cancel)?;
        // 在主工作树里是相对路径，相对于运行的目录。
        Ok(crate::host::resolve(
            root,
            String::from_utf8_lossy(&stdout).trim(),
        ))
    }

    /// 返回最近切换到过的分支名，最近的在前，不重复（来自 `HEAD` 的引用日志）。
    ///
    /// # Errors
    ///
    /// `git` 运行失败或超时时返回原因。
    pub fn recent_branches(&self, root: &Path, cancel: &Cancel) -> Result<Vec<String>, String> {
        let mut command = self.command(root);
        command.args([
            "log",
            "--walk-reflogs",
            "--format=%gs",
            "--grep-reflog=checkout: moving from",
            "--max-count=200",
            "HEAD",
        ]);
        let stdout = self.run(&mut command, STATUS_TIMEOUT, cancel)?;
        Ok(parse_recent_branches(&String::from_utf8_lossy(&stdout)))
    }

    /// 在仓库 `root` 里运行会改动仓库的 `git` 操作 `args`（如 `["pull"]`），返回 `git` 的结论。
    ///
    /// 分支名、提交号、储藏名等来自仓库或用户的参数之前必须加 `--end-of-options`，否则仓库里名为
    /// `--output=…` 的引用会被当作选项。
    ///
    /// # Errors
    ///
    /// 失败时返回 `git` 给出的原因：优先取 `error::`、`fatal::`、`! [rejected]` 这样的行。
    pub fn operate<S: AsRef<OsStr>>(&self, root: &Path, args: &[S]) -> Result<String, String> {
        if let Err(refused) = self.require_trust(root) {
            output::record(root, args, None, refused.as_bytes());
            return Err(refused);
        }
        self.run_operation(root, args, OPERATION_TIMEOUT)
    }

    /// 在后台无人值守地获取所有远程（`git fetch --all --prune`）：需要密码、口令或确认主机密钥
    /// 时直接失败，不等输入。
    ///
    /// `git` 自己的提示已经由 `GIT_TERMINAL_PROMPT` 关掉；`ssh` 还会从控制终端读口令（从终端
    /// 启动时），这里让它只用 askpass，而 askpass 程序是总是失败的 `false`（不存在时同样失败）。
    /// [`Git::with_askpass`] 指定的程序此时只作为凭据助手提供已保存的凭据。
    ///
    /// # Errors
    ///
    /// 同 [`Git::operate`]。
    pub fn fetch_unattended(&self, root: &Path) -> Result<String, String> {
        self.require_trust(root)?;
        let args = ["fetch", "--all", "--prune"];
        self.run_prepared(root, &args, OPERATION_TIMEOUT, |command| {
            command
                .env("GIT_ASKPASS", "false")
                .env("SSH_ASKPASS", "false")
                .env("SSH_ASKPASS_REQUIRE", "force");
        })
    }

    /// 把 `paths` 的当前状态加入暂存区（`git add --all`），已删除的文件记为删除。
    ///
    /// 路径以 `/` 分隔并按字面匹配：`[`、`*`、`?` 不作为通配符。
    ///
    /// # Errors
    ///
    /// 同 [`Git::operate`]。
    pub fn stage(&self, root: &Path, paths: &[PathBuf]) -> Result<String, String> {
        let mut args: Vec<OsString> = ["add", "--all", "--"].map(OsString::from).into();
        args.extend(relative_all(root, paths));
        self.operate(root, &args)
    }

    /// 储藏 `paths` 的改动（含未跟踪的文件），其余的改动留在工作区和暂存区；`message` 是储藏的
    /// 说明，为空时由 `git` 生成。
    ///
    /// 路径以 `/` 分隔并按字面匹配：`[`、`*`、`?` 不作为通配符。
    ///
    /// # Errors
    ///
    /// 同 [`Git::operate`]。
    pub fn stash_paths(
        &self,
        root: &Path,
        message: &str,
        paths: &[PathBuf],
    ) -> Result<String, String> {
        let mut args: Vec<OsString> = ["stash", "push", "--include-untracked"]
            .map(OsString::from)
            .into();
        if !message.is_empty() {
            args.extend(["--message", message].map(OsString::from));
        }
        args.push("--".into());
        args.extend(relative_all(root, paths));
        self.operate(root, &args)
    }

    /// 在目录 `root` 里运行 `git args`，记录输出，返回 `git` 的结论；调用方负责信任检查。
    fn run_operation<S: AsRef<OsStr>>(
        &self,
        root: &Path,
        args: &[S],
        timeout: Duration,
    ) -> Result<String, String> {
        self.run_prepared(root, args, timeout, |_| {})
    }

    /// 和 [`Git::run_operation`] 相同，运行前由 `prepare` 调整命令，例如设置环境变量。
    fn run_prepared<S: AsRef<OsStr>>(
        &self,
        root: &Path,
        args: &[S],
        timeout: Duration,
        prepare: impl FnOnce(&mut Command),
    ) -> Result<String, String> {
        let mut command = self.command(root);
        self.ask_through(root, &mut command);
        // 本机的 `git`（不是远端主机和 WSL 里的）在联网的命令上跟随系统的代理设置；暂存、提交
        // 这些不联网的命令不读系统设置，不为它们多跑一个进程。
        let online = online(args.iter().map(AsRef::as_ref));
        if online && !winc_backend::is_remote(root) && crate::wsl::locate_here(root).is_none() {
            crate::proxy::apply(&mut command);
        }
        prepare(&mut command);
        command.args(args).stderr(Stdio::piped());
        // 命令一开始就记下，输出随到随记：开着的输出标签不用等命令结束。
        let live = output::begin(root, args);
        let watch: process::Watch = {
            let live = Arc::clone(&live);
            Arc::new(move |bytes: &[u8]| output::feed(&live, bytes))
        };
        let cancel = Cancel::default();
        match process::run_watched(&mut command, Vec::new(), timeout, &cancel, Some(watch)) {
            Ok(output) => {
                let mut text = output.stderr;
                text.extend_from_slice(&output.stdout);
                output::finish(&live, output.status.code(), &text);
                if output.status.success() {
                    Ok(summary(&text))
                } else {
                    Err(failure(&text))
                }
            }
            Err(e) => {
                let reason = describe(&e);
                output::finish(&live, None, reason.as_bytes());
                Err(reason)
            }
        }
    }

    /// 返回使用 `askpass` 处理输入请求的副本；`None` 表示需要输入时直接失败。
    #[must_use]
    pub fn with_askpass(mut self, askpass: Option<Askpass>) -> Self {
        self.askpass = askpass;
        self
    }

    /// 为 `command` 设置 [`Git::with_askpass`] 指定的 askpass 程序。
    ///
    /// `git` 优先使用凭据助手，没有结果时才运行该程序。`SSH_ASKPASS_REQUIRE` 使 `ssh` 不依赖
    /// `DISPLAY` 和控制终端。WSL 发行版内的 `git` 经互操作运行该程序，`WSLENV` 另外列出
    /// [`crate::wsl::ASKPASS_ENV`]；发行版禁用了互操作时程序无法运行，需要输入时 `git` 报错。
    fn ask_through(&self, root: &Path, command: &mut Command) {
        let Some(askpass) = &self.askpass else {
            return;
        };
        // 远端的仓库：`git` 在那台主机上运行，它的输入请求由服务进程转回本机，再交给这里的
        // askpass 程序（见 `process::run`），这些变量本身不发到远端。
        command
            .env("GIT_ASKPASS", &askpass.program)
            .env("SSH_ASKPASS", &askpass.program)
            .env("SSH_ASKPASS_REQUIRE", "force")
            .env(ASKPASS_ENDPOINT, &askpass.endpoint);
        if crate::wsl::locate_here(root).is_some() {
            let list = format!("{}:{}", crate::wsl::GIT_ENV, crate::wsl::ASKPASS_ENV);
            command.env("WSLENV", list);
        } else if askpass.credential_helper && locate::runs_here(root) {
            locate::add_credential_helper(command, &askpass.program);
        }
    }

    /// 删除本地分支 `name`。
    ///
    /// `force` 时用 `--force` 连同未合并的提交一起删除；否则分支未完全合并时返回
    /// [`BranchDeleted::NeedsForce`]，不改动仓库。
    ///
    /// # Errors
    ///
    /// 分支不存在、被某个工作树签出等其他失败时返回 `git` 给出的原因。
    pub fn delete_branch(
        &self,
        root: &Path,
        name: &str,
        force: bool,
    ) -> Result<BranchDeleted, String> {
        let mut args = vec!["branch", "--delete"];
        if force {
            args.push("--force");
        }
        args.extend(["--end-of-options", name]);
        match self.operate(root, &args) {
            Ok(summary) => Ok(BranchDeleted::Done(summary)),
            // 报错固定为英文（`LC_ALL=C`），分支名不含空格，不会和这句话混淆。
            Err(e) if !force && e.contains("is not fully merged") => Ok(BranchDeleted::NeedsForce),
            Err(e) => Err(e),
        }
    }

    /// 推送当前分支。
    ///
    /// 没有上游时发布到 [`Git::publish_target`] 选出的远程（见 [`Git::publish`]）。
    ///
    /// # Errors
    ///
    /// 没有任何远程、`git` 报错（被拒绝、需要认证）时返回原因。
    pub fn push(&self, root: &Path, has_upstream: bool) -> Result<String, String> {
        self.require_trust(root)?;
        if has_upstream {
            return self.operate(root, &["push"]);
        }
        let remote = self.publish_target(root)?;
        self.publish(root, &remote)
    }

    /// 发布当前分支：推到远程 `remote` 上同名的分支，并设为上游（`push -u`）。
    ///
    /// # Errors
    ///
    /// `remote` 以 `-` 开头、`git` 报错（被拒绝、需要认证）时返回原因。
    pub fn publish(&self, root: &Path, remote: &str) -> Result<String, String> {
        self.require_trust(root)?;
        // 远程名来自仓库的配置，`git` 允许以 `-` 开头的远程名（`--receive-pack=…` 会运行命令）：
        // 拒绝这样的远程，推送时另外加 `--end-of-options`。
        if remote.starts_with('-') {
            return Err(winc_i18n::t!("git-push-no-remote").to_owned());
        }
        self.operate(
            root,
            &["push", "--set-upstream", "--end-of-options", remote, "HEAD"],
        )
    }

    /// 把当前分支推送到远程 `remote` 的分支 `branch`；`tags` 为真时一并推送全部标签，`track`
    /// 为真时把它设为上游。
    ///
    /// # Errors
    ///
    /// `remote`、`branch` 以 `-` 开头，或 `git` 报错（被拒绝、需要认证）时返回原因。
    pub fn push_to(
        &self,
        root: &Path,
        remote: &str,
        branch: &str,
        tags: bool,
        track: bool,
    ) -> Result<String, String> {
        self.require_trust(root)?;
        if remote.starts_with('-') || branch.starts_with('-') {
            return Err(winc_i18n::t!("git-push-no-remote").to_owned());
        }
        let spec = format!("HEAD:refs/heads/{branch}");
        let mut args = vec!["push"];
        if tags {
            args.push("--tags");
        }
        if track {
            args.push("--set-upstream");
        }
        args.extend(["--end-of-options", remote, &spec]);
        self.operate(root, &args)
    }

    /// 返回发布当前分支（第一次推送）时使用的远程：优先取分支和仓库配置的推送目标，其次是
    /// `origin`，都没有时取第一个；跳过以 `-` 开头的远程名。
    ///
    /// # Errors
    ///
    /// 没有可用的远程或 `git` 运行失败时返回原因。
    pub fn publish_target(&self, root: &Path) -> Result<String, String> {
        let remotes = self.remotes(root, &Cancel::default())?;
        let usable: Vec<&String> = remotes.iter().filter(|r| !r.starts_with('-')).collect();
        let preferred = self.preferred_remote(root);
        publish_remote(&usable, &preferred)
            .cloned()
            .ok_or_else(|| winc_i18n::t!("git-push-no-remote").to_owned())
    }

    /// 当前分支配置的推送目标，优先的在前，顺序和 `git push` 选择远程时相同；上游已不存在的分支
    /// 仍然留着 `branch.<名字>.remote`。
    fn preferred_remote(&self, root: &Path) -> Vec<String> {
        let cancel = Cancel::default();
        let mut command = self.command(root);
        command.args(["symbolic-ref", "--quiet", "--short", "HEAD"]);
        let branch = self.run(&mut command, STATUS_TIMEOUT, &cancel);
        let branch = branch.map(|out| String::from_utf8_lossy(&out).trim().to_owned());
        let branch = branch.unwrap_or_default();
        [
            format!("branch.{branch}.pushRemote"),
            "remote.pushDefault".to_owned(),
            format!("branch.{branch}.remote"),
        ]
        .iter()
        .filter_map(|key| self.config(root, key, &cancel))
        .collect()
    }

    fn command(&self, root: &Path) -> Command {
        let mut command = self.at(root);
        command
            // 不获取可选的锁（例如刷新索引）：后台运行的 `git` 不能和用户在同一仓库里运行的
            // `git` 抢同一把锁。
            .env("GIT_OPTIONAL_LOCKS", "0")
            // 禁用终端提示：没有 askpass 程序（见 `Git::ask_through`）时直接失败，不阻塞进程。
            .env("GIT_TERMINAL_PROMPT", "0")
            // 不打开编辑器：`rebase --continue` 等操作会打开它修改提交信息，这里沿用原来的信息。
            .env("GIT_EDITOR", ":")
            // 不验证签名：验证需要运行 `gpg.program`，仓库配置不可信时它可以是任意命令；验证结果
            // 还会混入 `log`、`show`、`stash list` 的输出，破坏解析。
            .args(["-c", "log.showSignature=false"]);
        // 配置不可信：fsmonitor 和仓库里定义的过滤器都不运行；必需的过滤器被清空命令后 `git` 会
        // 报错，不会把没经过过滤的内容当成结果。会改仓库的操作在此之前已经被拒绝，剩下的只读命令
        // 中只有 `diff.*.textconv` 会运行仓库配置的命令，`blame` 总是带 `--no-textconv`。
        if let Some(risks) = self.untrusted(root) {
            command.args(["-c", "core.fsmonitor=false"]);
            for name in &risks.filters {
                for key in ["clean", "smudge", "process"] {
                    command.arg("-c").arg(format!("filter.{name}.{key}="));
                }
            }
        }
        command
    }

    /// 返回在仓库 `root` 中运行 `git` 的命令，调用方继续追加 `git` 的参数。
    ///
    /// 发行版中的仓库使用发行版内的 `git`：跨文件系统读取仓库的速度较低，文件属主不同时 `git` 还会
    /// 按 `safe.directory` 拒绝访问。
    ///
    /// 环境中的 [`REPOSITORY_ENV`] 不传给 `git`：仓库由 `root` 决定，调用方需要时（例如临时暂存区）
    /// 在返回的命令上另行设置。
    pub(super) fn at(&self, root: &Path) -> Command {
        // 远端主机上的仓库：以仓库为工作目录运行远端的 `git`，按名字在远端的 `PATH` 中查找
        // （本机 `git` 的路径和本机 shell 的环境在远端没有意义）。命令经 `process::run` 在该
        // 目录所在的主机上运行。
        if winc_backend::is_remote(root) {
            let mut command = Command::new("git");
            command.current_dir(root);
            return command;
        }
        let mut command = match crate::wsl::locate_here(root) {
            None => {
                let mut command = self.shell.command(&self.program);
                command.arg("-C").arg(root);
                command
            }
            Some(place) => {
                let mut command = self.shell.command(Path::new("wsl.exe"));
                crate::wsl::enter(&mut command, &place.distro);
                command
                    .args(["--exec", "git", "-C", &place.path])
                    .env("WSLENV", crate::wsl::GIT_ENV);
                command
            }
        };
        for name in REPOSITORY_ENV {
            command.env_remove(name);
        }
        command
    }

    /// 返回文件在 `git` 中的模式：可执行文件为 `100755`，其余为 `100644`。
    ///
    /// Windows 无法读取发行版中文件的可执行位，因此在发行版内查询。查询失败时返回错误：按不可执行
    /// 处理会使提交移除文件的可执行位。
    fn file_mode(&self, path: &Path, cancel: &Cancel) -> Result<&'static str, String> {
        if let Some(place) = crate::wsl::locate_here(path) {
            let mut command = self.shell.command(Path::new("wsl.exe"));
            crate::wsl::enter(&mut command, &place.distro);
            command
                .args(["--exec", "test", "-x", &place.path])
                .stderr(Stdio::piped());
            let output = process::run(&mut command, Vec::new(), SHOW_TIMEOUT, cancel)
                .map_err(|e| describe(&e))?;
            return mode_from_test(output.status.code()).ok_or_else(|| failure(&output.stderr));
        }
        #[cfg(unix)]
        {
            let executable =
                |m: winc_backend::fs::Metadata| m.unix.is_some_and(|u| u.mode & 0o111 != 0);
            if winc_backend::fs::metadata(path).is_ok_and(executable) {
                return Ok("100755");
            }
        }
        Ok("100644")
    }

    fn run(
        &self,
        command: &mut Command,
        timeout: Duration,
        cancel: &Cancel,
    ) -> Result<Vec<u8>, String> {
        command.stderr(Stdio::piped());
        match process::run(command, Vec::new(), timeout, cancel) {
            Ok(output) if output.status.success() => Ok(output.stdout),
            Ok(output) => Err(failure(&output.stderr)),
            Err(e) => Err(describe(&e)),
        }
    }
}

/// 开着的 `git log` 进程，见 [`Git::log_stream`]。
///
/// 丢弃时连同 `git` 进程一起结束。
pub struct LogStream {
    stream: process::Stream,
    /// 按提交号找到的提交（[`Git::commit_by_hash`]），在第一批中排在最前面。
    pinned: Option<Box<Commit>>,
    /// 排在最前面的提交号；说明同样匹配时 `git log` 会再次输出该提交，读取时跳过。
    skip: Option<String>,
}

impl LogStream {
    /// 继续读最多 `max` 个提交；不满 `max` 个说明读完了。
    ///
    /// 每一批最多等 `LOG_TIMEOUT`。
    ///
    /// # Errors
    ///
    /// `git` 报错（还没有提交、分支不存在）、超时或读输出出错时返回原因。读取出错后该日志
    /// 不能再继续读。
    pub fn next(&mut self, max: usize) -> Result<Vec<Commit>, String> {
        let mut commits = self.read(max)?;
        if let Some(at) = commits
            .iter()
            .position(|c| Some(&c.oid) == self.skip.as_ref())
        {
            commits.remove(at);
            self.skip = None;
            // 本批数量少于 `max` 会被视为已读完，因此再读取一个补足。
            if commits.len() + 1 == max {
                commits.extend(self.read(1)?);
            }
        }
        if let Some(pinned) = self.pinned.take() {
            match commits.iter().position(|c| c.oid == pinned.oid) {
                Some(at) => {
                    commits.remove(at);
                }
                None => self.skip = Some(pinned.oid.clone()),
            }
            commits.insert(0, *pinned);
        }
        Ok(commits)
    }

    fn read(&mut self, max: usize) -> Result<Vec<Commit>, String> {
        let (bytes, _) = self
            .stream
            .read_records(0x1e, max, LOG_TIMEOUT)
            .map_err(|e| match e {
                process::StreamError::Run(e) => describe(&e),
                process::StreamError::Exited(stderr) => failure(&stderr),
            })?;
        Ok(parse_log(&String::from_utf8_lossy(&bytes)))
    }
}

/// `path` 相对仓库根目录 `root` 的路径，用 `/` 分隔（`git` 的路径分隔符总是 `/`）。
///
/// 各段按原样的字节拼接，不经有损转换：Unix 上文件名可以不是 UTF-8，替换成 U+FFFD 以后指向的
/// 是另一个（通常不存在的）文件。
fn slash_path(root: &Path, path: &Path) -> Result<OsString, String> {
    // 远端主机上的仓库：按远端的原始字节计算，虚拟路径中为本机所做的换算不能传给远端的 `git`。
    if let (Some(root), Some(path)) = (winc_backend::locate(root), winc_backend::locate(path)) {
        let relative = path
            .relative_to(&root)
            .ok_or_else(|| winc_i18n::t!("git-file-outside-repository"))?;
        #[cfg(unix)]
        return Ok(std::os::unix::ffi::OsStringExt::from_vec(relative.to_vec()));
        // Windows 的字符串无法表示不是有效 UTF-8 的文件名，这样的文件在远端的 `git` 中找不到。
        #[cfg(not(unix))]
        return Ok(String::from_utf8_lossy(relative).into_owned().into());
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| winc_i18n::t!("git-file-outside-repository"))?;
    Ok(slashed(relative))
}

/// 把相对路径 `relative` 的各段用 `/` 连接。
///
/// Windows 上的路径以 `\` 分隔，发行版内的 `git` 把 `\` 当作文件名中的普通字符，
/// `src\main.rs` 匹配不到 `src/main.rs`。
fn slashed(relative: &Path) -> OsString {
    let mut out = OsString::new();
    for (i, part) in relative.components().enumerate() {
        if i > 0 {
            out.push("/");
        }
        out.push(part.as_os_str());
    }
    out
}

/// 把 `parts` 依次接成一个参数，路径部分保持原样的字节（见 [`slash_path`]）。
fn joined_arg(parts: &[&OsStr]) -> OsString {
    parts.iter().fold(OsString::new(), |mut out, part| {
        out.push(part);
        out
    })
}

/// 把 `test -x` 的退出码转换为文件在 `git` 中的模式；其他退出码表示查询失败，返回 `None`。
fn mode_from_test(code: Option<i32>) -> Option<&'static str> {
    match code {
        Some(0) => Some("100755"),
        Some(1) => Some("100644"),
        _ => None,
    }
}

/// 返回 `paths` 对应的 `git` 路径参数（用于 `--` 之后）：位于 `root` 之下的路径换算为相对路径，
/// 各段以 `/` 连接（[`slashed`]）。
///
/// 每个都加上 `:(literal)`，按字面匹配：`app/[slug]/page.tsx` 里的 `[`、`*`、`?` 不当通配符，
/// 回滚、提交时不会连带改动名字碰巧匹配上的其他文件。不用 `GIT_LITERAL_PATHSPECS`：
/// `git stash push -u` 自己也要用路径匹配，开启它以后不再收录未跟踪的文件。
fn relative_all<'a>(
    root: &'a Path,
    paths: &'a [PathBuf],
) -> impl Iterator<Item = std::ffi::OsString> + 'a {
    paths.iter().map(move |p| {
        let mut spec = std::ffi::OsString::from(":(literal)");
        match p.strip_prefix(root) {
            Ok(relative) => spec.push(slashed(relative)),
            Err(_) => spec.push(p),
        }
        spec
    })
}

/// 会访问网络、需要代理的 `git` 子命令。
const ONLINE: [&str; 6] = ["fetch", "pull", "push", "clone", "ls-remote", "remote"];

/// `git args` 的子命令是否会访问网络（见 [`ONLINE`]）：跳过前面的 `-c 键=值` 和其他选项，看第一个
/// 子命令。
fn online<'a>(args: impl IntoIterator<Item = &'a OsStr>) -> bool {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "-c" {
            args.next();
            continue;
        }
        if arg.to_string_lossy().starts_with('-') {
            continue;
        }
        return ONLINE.iter().any(|command| arg == *command);
    }
    false
}

/// 选出发布分支（第一次推送）的远程：先按 `preferred`（当前分支和仓库配置的推送目标，优先的在前），
/// 其次是 `origin`，都没有时取 `remotes` 中的第一个；没有远程时返回 `None`。
///
/// 与 `git push` 一致：没有配置推送目标时推到 `origin`。远程按名字排列，审阅他人的拉取请求时
/// 添加的远程可能排在 `origin` 之前，不能直接取第一个。
fn publish_remote<'a>(remotes: &[&'a String], preferred: &[String]) -> Option<&'a String> {
    let named = |name: &str| {
        remotes
            .iter()
            .find(|remote| remote.as_str() == name)
            .copied()
    };
    preferred
        .iter()
        .find_map(|name| named(name))
        .or_else(|| named("origin"))
        .or_else(|| remotes.first().copied())
}

#[cfg(test)]
mod tests;
