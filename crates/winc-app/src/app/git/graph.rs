//! 提交视图底部的提交图：当前分支最近若干个提交连成的图，点一行打开该提交。
//!
//! 与 Git 日志标签里的提交图用的是同一套车道算法（[`winc_core::graph`]），这里只取最近
//! [`GRAPH_COMMITS`] 个提交，一次读完，不随日志标签滚动、筛选。读的是当前分支和对比的基准
//! （见 [`App::graph_refs`]），别的分支的提交不在图里。`HEAD`、分支或点击刷新后重新读取（见
//! [`App::reload_graph`]）；读取期间旧内容留着，避免图闪一下。
//!
//! 一屏放不下时上下滚动：滚轮、拖动右边细条（[`graph_rows`] 的右端），或者按住一行、按住空
//! 白处直接拖。图占多高可以按住上沿的分隔线上下调整（见 [`App::drag_graph_resize`]），调过
//! 的高度记在状态文件里。
//!
//! 标题行还写着与一个基准（默认是当前分支在远程上的那一份）的对比：领先、落后多少个提交，以
//! 及哪些行是基准没有的（见 [`GraphCompare`]）；基准独有的提交还没拉下来、本地独有的还没推送，
//! 两边独有的提交连同连着的线各涂一种颜色，一条线上哪段只在本地、哪段在线上一眼可辨。基准可
//! 以从标题行的对比控件换成任意本地分支或远程分支（见 [`App::graph_compare_base`]）。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::app::git::commit::DRAG_SLOP;
use winc_core::graph::{Graph, GraphRow};
use winc_editor::format::Cancel;
use winc_editor::git::{Commit, CommitFile, Divergence, LogQuery};
use winc_i18n::t;

use super::log::menus::local_diff;
use super::log::{Loaded, short};
use super::unusable;
use crate::app::input::context_menu::{MenuCommand, MenuEntry, MenuLabel};
use crate::app::{App, CommitTarget, Drag, Focus, Rect, UserEvent};

/// 提交视图里的提交图取多少个提交（个）。
///
/// 读的只是当前分支（和对比的基准），一屏放不下时上下滚动或把图拉高，所以取深一些也不会让
/// 读取变慢；更早的历史仍然到日志标签里看。
pub(crate) const GRAPH_COMMITS: usize = 200;

/// 提交视图里的提交图。
#[derive(Debug, Default)]
pub(crate) struct GraphPart {
    /// 取回来的提交，最近的在前。
    pub commits: Vec<Commit>,
    /// 每一行怎么画，与 `commits` 一一对应。
    pub rows: Vec<GraphRow>,
    /// 列表滚动了多少（逻辑像素）。
    pub scroll: f32,
    /// 图占多高（逻辑像素）；`None` 时按改动列表与储藏之间的空间过半决定（见
    /// [`Self::graph_section_height`]）。拖过上沿的分隔线以后就是拖出来的高度。
    pub height: Option<f32>,
    /// 正在读取。
    pub loading: bool,
    /// 读取失败的原因。
    pub error: Option<String>,
    /// 手动选的对比基准（分支、远程分支或标签名）；`None` 时用当前分支的上游自动对比。
    pub base: Option<String>,
    /// 与基准的对比结果；没有上游、也没手动选基准时为 `None`。
    pub compare: Option<GraphCompare>,
    /// 展开的提交（完整的提交号）：点了它，它改动的文件就地列在这一行下面。
    pub expanded: Option<String>,
    /// 展开的提交改动的文件；还在读取、读取失败时为空。
    pub files: Vec<CommitFile>,
    /// 展开的提交的文件还在读取。
    pub files_loading: bool,
    /// 读取文件的版本号：发出请求与返回不一致时丢弃结果。
    files_generation: u64,
    /// 上一次读取时的 `HEAD`（空仓库时是空串）：`HEAD` 不变的失败或空结果不重复读。
    head: Option<String>,
    /// 读取期间又要重新读取：读回的这一批已经过时，读完再读一次。
    again: bool,
    /// 当前这批的版本号：发出请求与返回不一致时丢弃结果。
    generation: u64,
    /// 对比的版本号：同上。
    compare_generation: u64,
}

/// 提交图与基准的对比：`HEAD` 与基准各自多出的提交。
///
/// 图里把 `HEAD` 独有的行标成领先（还没推送），把基准独有的行标成落后（还没拉到本地）；标题行
/// 的对比控件写着基准的名字和两边的个数。
#[derive(Debug, Clone, Default)]
pub(crate) struct GraphCompare {
    /// 基准的名字：`origin/dev` 这样的远程分支、本地分支或标签。
    pub base: String,
    /// 基准缺少的提交号（`HEAD` 独有）。
    pub ahead: HashSet<String>,
    /// `HEAD` 缺少的提交号（基准独有）。
    pub behind: HashSet<String>,
    /// 领先、落后多少个提交（不受 [`winc_editor::git::COMPARE_LIMIT`] 限制）。
    pub counts: (usize, usize),
}

impl GraphCompare {
    /// 提交 `oid` 在对比中的位置：`Some(true)` 领先、`Some(false)` 落后、`None` 两边都有。
    pub(crate) fn side(&self, oid: &str) -> Option<bool> {
        if self.ahead.contains(oid) {
            return Some(true);
        }
        self.behind.contains(oid).then_some(false)
    }
}

impl GraphPart {
    /// 开始重新读取，返回这一批的版本号；旧内容留着，读到后整个换掉。
    fn begin(&mut self) -> u64 {
        self.generation += 1;
        self.loading = true;
        self.error = None;
        self.generation
    }

    /// 有没有可以画的内容。
    pub(crate) fn is_empty(&self) -> bool {
        self.commits.is_empty()
    }

    /// 要不要（重新）读一次图：还没读过，或者读的还是别的 `HEAD`。
    ///
    /// 空仓库和读取出错也算读过（[`Self::head`] 记的是尝试时的 `HEAD`）：`HEAD` 不变时不重复读，
    /// 否则每次刷新状态都会再跑一遍 `git log`。手动刷新与切回提交视图会直接读一次。
    pub(crate) fn stale(&self, head: Option<&str>) -> bool {
        self.head.as_deref() != Some(head.unwrap_or_default())
    }

    /// 第 `index` 个提交的提交号。
    pub(crate) fn oid(&self, index: usize) -> Option<&str> {
        self.commits.get(index).map(|c| c.oid.as_str())
    }
}

impl App {
    /// 重新读取提交视图里的提交图（`HEAD` 变化、点击刷新、切换到提交视图时）。
    ///
    /// 正在读取时不再发出请求：当前这一批读完后按那时的 `HEAD` 再读一次。对比与图一起读取（见
    /// [`Self::reload_compare`]）：图的基准取自同一时刻的分支列表。
    ///
    /// 读的是当前分支和对比的基准（见 [`Self::graph_refs`]）最近 [`GRAPH_COMMITS`] 个提交：
    /// 本地和线上两边都在图里，领先、落后的箭头才标得出来；别的分支的提交不混进来。要看得更远
    /// 就打开日志标签。
    pub(in crate::app) fn reload_graph(&mut self) {
        let Some(root) = self.git.root.clone() else {
            self.git.graph = GraphPart::default();
            return;
        };
        if self.git.graph.loading {
            // 这一批读完之前 `HEAD` 又动了：读回的结果已经过时，读完按新的 `HEAD` 再读一次。
            self.git.graph.again = true;
            return;
        }
        let head = self.git.status.as_ref().and_then(|s| s.oid.clone());
        self.git.graph.head = Some(head.unwrap_or_default());
        let generation = self.git.graph.begin();
        let query = LogQuery {
            branches: self.graph_refs(),
            ..LogQuery::default()
        };
        let sender = self.git.log.results.0.clone();
        self.spawn_git(UserEvent::GitLog, move |tool| {
            let read = |git: winc_editor::git::Git| {
                let mut stream = git.log_stream(&root, &query)?;
                stream.next(GRAPH_COMMITS)
            };
            let result = tool.map_err(|e| unusable(&e)).and_then(read);
            let _ = sender.send(Loaded::Graph { generation, result });
        });
        self.reload_compare();
    }

    /// 后台读回的提交图（见 [`Loaded::Graph`]）：算好每行的车道后换上。
    pub(in crate::app) fn graph_arrived(
        &mut self,
        generation: u64,
        result: Result<Vec<Commit>, String>,
    ) {
        // 已经重新发起读取时丢下过时的结果。
        if generation != self.git.graph.generation {
            return;
        }
        self.git.graph.loading = false;
        match result {
            Ok(commits) => {
                let mut lanes = Graph::new();
                self.git.graph.rows = commits
                    .iter()
                    .map(|commit| lanes.push(&commit.oid, &commit.parents))
                    .collect();
                self.git.graph.commits = commits;
                self.git.graph.error = None;
                self.git.graph.scroll = self.git.graph.scroll.max(0.0);
                // 图重读后展开的提交可能已经不在列表里（改写历史、切换分支）：收起。
                if let Some(oid) = &self.git.graph.expanded
                    && !self.git.graph.commits.iter().any(|c| &c.oid == oid)
                {
                    self.git.graph.expanded = None;
                    self.git.graph.files = Vec::new();
                    self.git.graph.files_loading = false;
                }
            }
            Err(error) => self.git.graph.error = Some(error),
        }
        if std::mem::take(&mut self.git.graph.again) {
            self.reload_graph();
        }
        self.request_redraw();
    }

    /// 点中提交图里的第 `index` 行：展开（或收起）它改动的文件，文件就地列在这一行下面，点
    /// 其中的文件直接打开。
    ///
    /// 按下时只记住起点（见 [`Self::begin_graph_pan`]），松开时没拖动过才走到这里。
    pub(in crate::app) fn press_graph_row(&mut self, index: usize) {
        let Some(oid) = self.git.graph.oid(index) else {
            return;
        };
        let oid = oid.to_owned();
        // 再点一次同一行：收起。
        if self.git.graph.expanded.as_deref() == Some(oid.as_str()) {
            self.collapse_graph_files();
            return;
        }
        self.expand_graph_commit(&oid);
    }

    /// 展开提交 `oid`：先记下展开状态并清掉上一次的文件，后台读取它改动的文件。
    pub(in crate::app) fn expand_graph_commit(&mut self, oid: &str) {
        let Some(root) = self.git.root.clone() else {
            return;
        };
        // 文件对照第一个父提交算（根提交与空树比），与日志详情一致。
        let parent = self
            .graph_commit(oid)
            .and_then(|c| c.parents.first().cloned());
        let oid = oid.to_owned();
        self.git.graph.expanded = Some(oid.clone());
        self.git.graph.files = Vec::new();
        self.git.graph.files_loading = true;
        self.git.graph.files_generation += 1;
        let generation = self.git.graph.files_generation;
        let sender = self.git.log.results.0.clone();
        self.spawn_git(UserEvent::GitLog, move |tool| {
            let read = |git: winc_editor::git::Git| {
                git.commit_files(&root, parent.as_deref(), &oid, &Cancel::default())
            };
            let result = tool.map_err(|e| unusable(&e)).and_then(read);
            let _ = sender.send(Loaded::GraphFiles {
                generation,
                oid,
                result,
            });
        });
        self.request_redraw();
    }

    /// 收起展开的文件列表。
    pub(in crate::app) fn collapse_graph_files(&mut self) {
        self.git.graph.expanded = None;
        self.git.graph.files = Vec::new();
        self.git.graph.files_loading = false;
        self.request_redraw();
    }

    /// 后台读回的展开提交的文件列表（见 [`Loaded::GraphFiles`]）：换上；已经展开别的提交
    /// （版本号不一致）时丢弃。
    pub(in crate::app) fn graph_files_arrived(
        &mut self,
        generation: u64,
        _oid: String,
        result: Result<Vec<CommitFile>, String>,
    ) {
        if generation != self.git.graph.files_generation {
            return;
        }
        self.git.graph.files_loading = false;
        if let Ok(files) = result {
            self.git.graph.files = files;
        }
        self.request_redraw();
    }

    /// 点中展开列表里的第 `index` 个文件：文件在工作区里就直接打开它（当前内容），方便查看；
    /// 已被删除的到日志里看这个提交的差异。
    pub(in crate::app) fn press_graph_file(&mut self, index: usize) {
        let Some(path) = self.git.graph.files.get(index).map(|f| f.path.clone()) else {
            return;
        };
        if self.may_be_file(&path) {
            self.open_path(&path);
            self.focus = Focus::Editor;
        } else if let Some(oid) = self.git.graph.expanded.clone() {
            self.show_log_commit(&oid, false);
        }
        self.request_redraw();
    }

    /// 提交图里提交号是 `oid` 的提交；日志里的提交菜单在日志没有这个提交时也回退到这里找。
    pub(crate) fn graph_commit(&self, oid: &str) -> Option<Commit> {
        self.git
            .graph
            .commits
            .iter()
            .find(|c| c.oid == oid)
            .cloned()
    }

    /// 展开的文件列表里第 `index` 个文件显示的文字：路径相对于打开的目录，改了名的按
    /// 「旧名 → 新名」写。
    pub(crate) fn graph_file_label(&self, index: usize) -> Option<String> {
        let file = self.git.graph.files.get(index)?;
        let name = |path: &Path| {
            self.relative_path(path)
                .unwrap_or_else(|| winc_editor::display_name(path).into_owned())
        };
        let mut text = name(&file.path);
        if let Some(from) = &file.from {
            text = format!("{} → {}", name(from), text);
        }
        Some(text)
    }

    /// 提交图读哪些引用：当前分支，加上对比的基准。
    ///
    /// 基准那一侧独有的提交（还没拉下来的）也在图里，行前的向下箭头才有对应的行；没有基准时只
    /// 读当前分支。游离的 `HEAD`、还没有任何分支时按 `HEAD` 读，仍然是签出的那一段历史。
    pub(crate) fn graph_refs(&self) -> Vec<String> {
        let mut refs: Vec<String> = Vec::new();
        if let Some(current) = self.git.branches.iter().find(|b| b.current) {
            refs.push(current.name.clone());
        }
        if let Some(base) = self.graph_compare_base()
            && !refs.contains(&base)
        {
            refs.push(base);
        }
        if refs.is_empty() {
            refs.push("HEAD".to_owned());
        }
        refs
    }

    /// 自动对比的基准：当前分支在远程上的那一份（`origin/dev` 这样）。
    ///
    /// 先看分支配置的上游；没有配置上游、但远程上有同名分支时（`git push` 之后没有跟踪上的、
    /// 从别的机器克隆后手动建的本地分支）也拿它比，本地与线上差多少照样看得出来。两边都没有时
    /// 返回 `None`，图里不显示对比。
    pub(crate) fn graph_upstream(&self) -> Option<String> {
        let current = self.git.branches.iter().find(|b| b.current)?;
        if let Some(upstream) = &current.upstream {
            return Some(upstream.clone());
        }
        // 没有上游：同名分支在某个远程上就用它，远程的顺序按 `git remote` 的顺序。
        let branch = current.name.as_str();
        self.git
            .remotes
            .iter()
            .map(|remote| format!("{remote}/{branch}"))
            .find(|name| {
                self.git
                    .branches
                    .iter()
                    .any(|b| b.remote && &b.name == name)
            })
    }

    /// 对比用的基准：手动选的，否则是当前分支的上游（见 [`Self::graph_upstream`]）。
    ///
    /// 两种情况都没有时返回 `None`，图里不显示对比。
    pub(crate) fn graph_compare_base(&self) -> Option<String> {
        self.git
            .graph
            .base
            .clone()
            .or_else(|| self.graph_upstream())
    }

    /// 重新读取提交图与基准的对比（每次重新读取图时，以及换了基准以后）。
    ///
    /// 没有基准时清掉上一个结果；读取失败时也清掉，图里就不画标记，不另外提示。
    pub(in crate::app) fn reload_compare(&mut self) {
        let base = self.graph_compare_base();
        let Some((root, base)) = self.git.root.clone().zip(base) else {
            self.git.graph.compare = None;
            return;
        };
        self.git.graph.compare_generation += 1;
        let generation = self.git.graph.compare_generation;
        let sender = self.git.log.results.0.clone();
        self.spawn_git(UserEvent::GitLog, move |tool| {
            let result = tool
                .map_err(|e| unusable(&e))
                .and_then(|git| git.compare(&root, &base));
            let _ = sender.send(Loaded::GraphCompare {
                generation,
                base,
                result,
            });
        });
    }

    /// 后台读回的对比（见 [`Loaded::GraphCompare`]）：记下哪些行领先、哪些行落后。
    pub(in crate::app) fn graph_compare_arrived(
        &mut self,
        generation: u64,
        base: String,
        result: Result<Divergence, String>,
    ) {
        // 已经重新发起读取时丢下过时的结果。
        if generation != self.git.graph.compare_generation {
            return;
        }
        self.git.graph.compare = result.ok().map(|diverged| GraphCompare {
            base,
            ahead: diverged.ahead.into_iter().collect(),
            behind: diverged.behind.into_iter().collect(),
            counts: (diverged.ahead_count, diverged.behind_count),
        });
        self.request_redraw();
    }

    /// 提交图标题行上的按钮、对比控件的悬停提示：不能用的写明原因（没有远程仓库、正在做别的
    /// 操作）。
    pub(crate) fn graph_tip(&self, target: CommitTarget) -> String {
        let upstream = self.graph_upstream();
        let busy = self.git.branch.running.as_deref();
        if target != CommitTarget::GraphCompare {
            if let Some(task) = busy {
                return t!("workspace-status-branch-running", task = task);
            }
            if self.git.remotes.is_empty() {
                return t!("git-graph-no-remote").to_owned();
            }
        }
        match target {
            CommitTarget::GraphFetch => t!("git-graph-fetch-tip").to_owned(),
            CommitTarget::GraphPull | CommitTarget::GraphPush => match upstream {
                Some(upstream) if target == CommitTarget::GraphPull => {
                    t!("git-graph-pull-tip", upstream = upstream)
                }
                Some(upstream) => t!("git-graph-push-tip", upstream = upstream),
                None => t!("git-graph-no-upstream").to_owned(),
            },
            _ => match &self.git.graph.compare {
                Some(compare) if compare.counts == (0, 0) => {
                    t!("git-graph-compare-same", base = compare.base.as_str())
                }
                Some(compare) => t!(
                    "git-graph-compare-tip",
                    base = compare.base.as_str(),
                    ahead = compare.counts.0,
                    behind = compare.counts.1
                ),
                None => t!("git-graph-compare-none").to_owned(),
            },
        }
    }

    /// 在日志标签里比较当前分支与对比的基准：改动的文件、逐个的差异。与分支菜单里的
    /// 「与当前分支比较」打开的是同一页。
    pub(in crate::app) fn graph_compare_in_log(&mut self) {
        if let Some(base) = self.graph_compare_base() {
            self.git_compare_branch(&base);
        }
    }

    /// 换对比的基准：`None` 是回到自动对比上游。
    pub(in crate::app) fn set_graph_compare_base(&mut self, base: Option<String>) {
        if self.git.graph.base == base {
            return;
        }
        self.git.graph.base = base;
        // 图里读的引用跟着变（见 [`Self::graph_refs`]）：新的基准独有、当前分支没有的提交要
        // 出现在图里，换掉以后重新读一遍。
        self.reload_graph();
        self.request_redraw();
    }

    /// 提交图占的高度（逻辑像素）：拖过就是拖出来的高度，没拖过时是 [`GRAPH_HEIGHT`]。
    ///
    /// 侧栏太矮时不显示提交图，改动列表优先。
    pub(crate) fn graph_section_height(&self, room: f32) -> Option<f32> {
        if self.git.graph.is_empty() || room < GRAPH_MIN_ROOM + GRAPH_MIN {
            return None;
        }
        let want = self.git.graph.height.unwrap_or(GRAPH_HEIGHT);
        Some(want.clamp(GRAPH_MIN, room - GRAPH_MIN_ROOM))
    }

    /// 启动时按状态文件恢复提交图的高度。
    pub(crate) fn restore_graph_height(&mut self) {
        self.git.graph.height = self.state.graph_height;
    }

    /// 拖动结束：把提交图的高度记进状态文件，下次启动沿用。
    pub(in crate::app) fn remember_graph_height(&mut self) {
        let height = self.git.graph.height.map(|height| height.round());
        self.update_state(|state| state.graph_height = height);
    }

    /// 正在拖动提交图的上沿调整高度。
    pub(crate) fn graph_resizing(&self) -> bool {
        matches!(self.pointer.drag, Drag::GraphResize { .. })
    }

    /// 按住提交图的上沿开始调整高度。
    pub(in crate::app) fn begin_graph_resize(&mut self) {
        let current = self.commit_parts().graph.map_or(0.0, |area| area.h);
        self.settle_glides();
        self.pointer.drag = Drag::GraphResize {
            start_y: self.pointer.mouse.1,
            height: current,
        };
    }

    /// 拖动提交图的上沿：往上拉图变高，往下拉图变矮，两端有界（改动列表至少留
    /// [`GRAPH_MIN_ROOM`]，图自己至少 [`GRAPH_MIN`]）。
    pub(in crate::app) fn drag_graph_resize(&mut self, start_y: f32, height: f32, y: f32) {
        let parts = self.commit_parts();
        let Some(graph) = parts.graph else {
            return;
        };
        // 改动列表和提交图共用的那段空间：上沿上下移动时，列表按剩下的高度重排。
        let room = graph.y + graph.h - parts.list.y;
        let limit = (room - GRAPH_MIN_ROOM).max(GRAPH_MIN);
        let height = (height - (y - start_y)).clamp(GRAPH_MIN, limit);
        if self.git.graph.height == Some(height) {
            return;
        }
        self.git.graph.height = Some(height);
        self.request_redraw();
    }

    /// 提交图最多能滚动多远（逻辑像素）：提交的行数加上展开列表的行数，乘以行高，减去
    /// `rows`（[`graph_rows`] 的结果）里看得到的那部分。
    pub(crate) fn graph_scroll_max(&self, rows: Rect) -> f32 {
        let count = self.git.graph.commits.len()
            + match self.git.graph.expanded.as_deref() {
                // 正在读取时先留出一行「正在读取」的提示。
                Some(_) => self.git.graph.files.len() + usize::from(self.git.graph.files_loading),
                None => 0,
            };
        ((count as f32) * GRAPH_ROW - rows.h.max(0.0)).max(0.0)
    }

    /// 在提交图上按下第 `row` 行：开始上下拖动，内容跟着指针走。
    ///
    /// 拖动也从行上开始，按下时不打开提交；松开时移动不到 [`DRAG_SLOP`] 才当点击处理
    /// （见 [`Self::release_graph_pan`]）。
    pub(in crate::app) fn begin_graph_pan(&mut self, row: Option<usize>) {
        self.settle_glides();
        self.pointer.drag = Drag::GraphPan {
            start: self.pointer.mouse,
            scroll: self.git.graph.scroll,
            row,
        };
    }

    /// 拖动提交图：指针移动多少，内容就跟着走多少，限制在两端之间。
    pub(in crate::app) fn drag_graph(&mut self, start: (f32, f32), scroll: f32, y: f32) {
        let max = self
            .commit_parts()
            .graph
            .map_or(0.0, |area| self.graph_scroll_max(graph_rows(area)));
        self.git.graph.scroll = (scroll - (y - start.1)).clamp(0.0, max);
        self.request_redraw();
    }

    /// 松开提交图上的拖动：移动不到 [`DRAG_SLOP`] 时按点击处理，展开按下的那一行。
    ///
    /// 拖过一段距离时停在这里，不回到整行：与滚轮一样按像素滚动，行可以停在屏幕中间。
    pub(in crate::app) fn release_graph_pan(&mut self, start: (f32, f32), row: Option<usize>) {
        let (_, y) = self.pointer.mouse;
        if (y - start.1).abs() > DRAG_SLOP {
            return;
        }
        if let Some(row) = row {
            self.press_graph_row(row);
        }
    }

    /// 右键点在提交图的一行提交上：返回它的提交号。
    pub(in crate::app) fn graph_context_target(&mut self, x: f32, y: f32) -> Option<String> {
        if !self.commit_shown() {
            return None;
        }
        let CommitTarget::GraphRow(i) = crate::layout::hit(&self.hits.commit, x, y)? else {
            return None;
        };
        self.focus = Focus::CommitList;
        let oid = self.git.graph.oid(i)?.to_owned();
        Some(oid)
    }

    /// 右键点在提交图展开的文件列表里：返回（提交号，文件的路径）。
    pub(in crate::app) fn graph_file_context_target(
        &mut self,
        x: f32,
        y: f32,
    ) -> Option<(String, PathBuf)> {
        if !self.commit_shown() {
            return None;
        }
        let CommitTarget::GraphFile(i) = crate::layout::hit(&self.hits.commit, x, y)? else {
            return None;
        };
        let path = self.git.graph.files.get(i)?.path.clone();
        Some((self.git.graph.expanded.clone()?, path))
    }

    /// 提交图提交菜单里的命令：展开、收起、在日志中查看是提交图自己的，其余与日志的提交命令
    /// 共用一套（拣选、还原、复制提交号与说明……）。
    pub(in crate::app) fn run_graph_commit_command(&mut self, command: MenuCommand, oid: &str) {
        match command {
            MenuCommand::GraphShowInLog => self.show_log_commit(oid, false),
            MenuCommand::GraphOpenFiles => self.expand_graph_commit(oid),
            MenuCommand::GraphHideFiles => {
                if self.git.graph.expanded.as_deref() == Some(oid) {
                    self.collapse_graph_files();
                }
            }
            command => self.run_log_commit_command(command, oid),
        }
        self.request_redraw();
    }

    /// 提交图展开列表里一个文件的右键菜单命令：打开当前文件、与工作区比较、文件历史、显示
    /// 位置、复制路径。
    pub(in crate::app) fn run_graph_file_command(
        &mut self,
        command: MenuCommand,
        oid: &str,
        path: PathBuf,
    ) {
        match command {
            MenuCommand::GitOpenFile => {
                if self.may_be_file(&path) {
                    self.open_path(&path);
                    self.focus = Focus::Editor;
                }
            }
            // 文件在这个提交里的版本和工作区里的版本并排比较。
            MenuCommand::LogCompareWithLocal => self.compare_graph_file_with_local(oid, path),
            MenuCommand::FileHistory => self.git_file_history(&path),
            MenuCommand::CopyPath | MenuCommand::CopyRelativePath => {
                self.copy_path(&path, command == MenuCommand::CopyRelativePath);
            }
            MenuCommand::Reveal => self.reveal_in_file_manager(&path),
            MenuCommand::RevealInTree => {
                self.show_files();
                self.reveal_path(&path);
            }
            _ => {}
        }
        self.request_redraw();
    }

    /// 把文件 `path` 在提交 `oid` 里的版本和工作区里的版本并排比较，在后台读取，打开为新
    /// 标签（与日志的「与工作区比较」打开的是同一类标签，见 [`Loaded::LocalDiff`]）。
    pub(in crate::app) fn compare_graph_file_with_local(&mut self, oid: &str, path: PathBuf) {
        let Some(root) = self.git.root.clone() else {
            return;
        };
        let rev = oid.to_owned();
        let name = winc_editor::display_name(&path).into_owned();
        let title = t!(
            "git-log-local-diff-title",
            name = name.as_str(),
            rev = short(&rev)
        );
        let sender = self.git.log.results.0.clone();
        self.spawn_git(UserEvent::GitLog, move |tool| {
            let view = tool
                .map_err(|e| unusable(&e))
                .and_then(|git| local_diff(&git, &root, &rev, &path))
                .map(Box::new);
            let _ = sender.send(Loaded::LocalDiff { title, view });
        });
    }
}

/// 提交图里画行的区域：去掉上沿标题行，滚动条和命中的范围都按这一块算（逻辑像素）。
pub(crate) fn graph_rows(area: Rect) -> Rect {
    Rect::new(
        area.x,
        area.y + GRAPH_HEADER,
        area.w,
        (area.h - GRAPH_HEADER).max(0.0),
    )
}

/// 提交图对比控件的菜单：回到自动对比上游，然后是其余的本地分支、远程分支两组。
///
/// `branches` 是 [`crate::app::git::GitState::branches`]（本地分支在前），下标就是
/// [`MenuCommand::GraphCompareBase`] 的参数；当前分支不比，`auto` 是自动对比用的上游（没有
/// 上游时那一项灰掉）。
pub(crate) fn compare_menu(
    branches: &[winc_editor::git::Branch],
    auto: Option<&str>,
) -> Vec<MenuEntry> {
    use MenuCommand::{GitHeading, GraphCompareAuto, GraphCompareBase, GraphCompareLog};
    use MenuEntry::{Item, Separator};

    let mut menu = vec![Item(GraphCompareAuto, auto.is_some())];
    for remote in [false, true] {
        let group: Vec<usize> = (0..branches.len())
            .filter(|&i| branches[i].remote == remote && !branches[i].current)
            .collect();
        if group.is_empty() {
            continue;
        }
        menu.push(Separator);
        menu.push(Item(GitHeading(remote), false));
        menu.extend(group.into_iter().map(|i| Item(GraphCompareBase(i), true)));
    }
    // 直接在日志标签里比较两个版本改动的文件：和图上的领先、落后是同一对版本。
    menu.push(Separator);
    menu.push(Item(GraphCompareLog, true));
    menu
}

/// 提交图对比控件的菜单，见 [`compare_menu`]。
impl App {
    /// 对比控件弹出的菜单，见 [`compare_menu`]。
    pub(in crate::app) fn compare_menu(&self) -> Vec<MenuEntry> {
        compare_menu(&self.git.branches, self.graph_upstream().as_deref())
    }

    /// 对比菜单里命令的文案：基准写分支名，「自动」写明上游。
    pub(in crate::app) fn compare_menu_label(&self, command: MenuCommand) -> MenuLabel {
        match command {
            MenuCommand::GraphCompareBase(i) => MenuLabel::plain(
                self.git
                    .branches
                    .get(i)
                    .map_or("", |branch| branch.name.as_str()),
            ),
            _ => {
                let label = match self.graph_upstream() {
                    Some(upstream) => t!("git-graph-compare-auto", upstream = upstream),
                    None => t!("git-graph-compare-auto-none").to_owned(),
                };
                MenuLabel::plain(&label)
            }
        }
    }

    /// 对比菜单里的勾：换了基准时勾那一项，没换（自动对比上游）时勾「自动」。
    pub(in crate::app) fn compare_menu_checked(&self, command: MenuCommand) -> bool {
        match command {
            MenuCommand::GraphCompareAuto => self.git.graph.base.is_none(),
            MenuCommand::GraphCompareBase(i) => {
                let name = self.git.branches.get(i).map(|b| b.name.as_str());
                name.is_some() && self.git.graph.base.as_deref() == name
            }
            _ => false,
        }
    }
}

/// 没有调整过时提交图占的高度（逻辑像素）：一行标题加十三行提交，一屏能看到半个月的工作。
///
/// 固定高度而不是按比例：提交信息框长高、储藏展开时改动列表照常伸缩，提交图不跟着变，列表里
/// 哪一行在什么位置也就不会跳。拖过上沿的分隔线以后按拖出来的高度（见
/// [`App::graph_section_height`]）。
const GRAPH_HEIGHT: f32 = 300.0;
/// 提交图上沿可以用来拖动的那一段的高度（逻辑像素）：分隔线本身只有一像素，光标停在附近就能
/// 按下去。
pub(crate) const GRAPH_GRIP: f32 = 6.0;
/// 改动列表至少留这么高，放不下提交图时（窗口很矮）不显示提交图（逻辑像素）。
const GRAPH_MIN_ROOM: f32 = 140.0;
/// 提交图自己至少要这么高才显示（逻辑像素）：再矮就只剩标题，不如不显示。
pub(crate) const GRAPH_MIN: f32 = 120.0;
/// 提交图顶部标题行的高度（逻辑像素）。
pub(crate) const GRAPH_HEADER: f32 = 24.0;
/// 提交图一行的行高（逻辑像素）。
pub(crate) const GRAPH_ROW: f32 = 20.0;
