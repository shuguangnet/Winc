//! 提交视图底部的提交图：最近若干个提交连成的图，点一行打开该提交。
//!
//! 与 Git 日志标签里的提交图用的是同一套车道算法（[`winc_core::graph`]），这里只取最近
//! [`GRAPH_COMMITS`] 个提交，一次读完，不随日志标签滚动、筛选。`HEAD`、分支或点击刷新后重新
//! 读取（见 [`App::reload_graph`]）；读取期间旧内容留着，避免图闪一下。
//!
//! 一屏放不下时上下滚动：滚轮、拖动右边细条（[`graph_rows`] 的右端），或者按住一行直接拖。
//!
//! 标题行还写着与一个基准（默认是当前分支的上游）的对比：领先、落后多少个提交，以及哪些行
//! 是基准没有的（见 [`GraphCompare`]）。基准可以从标题行的对比控件换成任意本地分支或远程分支
//! （见 [`App::graph_compare_base`]）。

use std::collections::HashSet;

use winc_core::graph::{Graph, GraphRow};
use winc_editor::git::{Commit, Divergence, LogQuery};
use winc_i18n::t;

use super::log::Loaded;
use super::unusable;
use crate::app::input::context_menu::{MenuCommand, MenuEntry, MenuLabel};
use crate::app::{App, CommitTarget, Drag, Rect, UserEvent};

/// 提交视图里的提交图取多少个提交（个）。
///
/// 侧栏的图是给「最近做了什么」看的，与日志标签不同，不必读很深；一屏放不下时可以滚动。
pub(crate) const GRAPH_COMMITS: usize = 60;

/// 提交视图里的提交图。
#[derive(Debug, Default)]
pub(crate) struct GraphPart {
    /// 取回来的提交，最近的在前。
    pub commits: Vec<Commit>,
    /// 每一行怎么画，与 `commits` 一一对应。
    pub rows: Vec<GraphRow>,
    /// 列表滚动了多少（逻辑像素）。
    pub scroll: f32,
    /// 正在读取。
    pub loading: bool,
    /// 读取失败的原因。
    pub error: Option<String>,
    /// 手动选的对比基准（分支、远程分支或标签名）；`None` 时用当前分支的上游自动对比。
    pub base: Option<String>,
    /// 与基准的对比结果；没有上游、也没手动选基准时为 `None`。
    pub compare: Option<GraphCompare>,
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

    /// 第 `index` 个提交的提交号。
    pub(crate) fn oid(&self, index: usize) -> Option<&str> {
        self.commits.get(index).map(|c| c.oid.as_str())
    }
}

impl App {
    /// 重新读取提交视图里的提交图（`HEAD` 变化、点击刷新、切换到提交视图时）。
    ///
    /// 正在读取时不再发出请求：当前这一批读完后按那时的 `HEAD` 生效。对比与图一起读取（见
    /// [`Self::reload_compare`]）：图的基准取自同一时刻的分支列表。
    pub(in crate::app) fn reload_graph(&mut self) {
        let Some(root) = self.git.root.clone() else {
            self.git.graph = GraphPart::default();
            return;
        };
        if self.git.graph.loading {
            return;
        }
        let generation = self.git.graph.begin();
        let sender = self.git.log.results.0.clone();
        self.spawn_git(UserEvent::GitLog, move |tool| {
            let read = |git: winc_editor::git::Git| {
                let mut stream = git.log_stream(&root, &LogQuery::default())?;
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
            }
            Err(error) => self.git.graph.error = Some(error),
        }
        self.request_redraw();
    }

    /// 点中提交图里的第 `index` 行：在 Git 日志标签中打开这个提交。
    ///
    /// 按下时只记住起点（见 [`Self::begin_graph_pan`]），松开时没拖动过才走到这里。
    pub(in crate::app) fn press_graph_row(&mut self, index: usize) {
        let Some(oid) = self.git.graph.oid(index) else {
            return;
        };
        let oid = oid.to_owned();
        self.show_log_commit(&oid, false);
    }

    /// 当前分支的上游（`origin/dev` 这样）：自动对比用的就是它。
    ///
    /// 游离的 `HEAD`、分支还没发布时返回 `None`。
    pub(crate) fn graph_upstream(&self) -> Option<String> {
        (self.git.branches.iter())
            .find(|b| b.current)
            .and_then(|b| b.upstream.clone())
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
        self.reload_compare();
        self.request_redraw();
    }

    /// 提交图内容的高度下界、上界，以及改动列表至少留多高（逻辑像素）。
    ///
    /// 侧栏太矮时不显示提交图，改动列表优先。
    pub(crate) fn graph_section_height(&self, room: f32) -> Option<f32> {
        if self.git.graph.is_empty() || room < GRAPH_MIN_ROOM + GRAPH_MIN {
            return None;
        }
        Some(GRAPH_HEIGHT.min(room - GRAPH_MIN_ROOM))
    }

    /// 提交图最多能滚动多远（逻辑像素）：行数乘以行高，减去 `rows`（[`graph_rows`] 的结果）
    /// 里看得到的那部分。
    pub(crate) fn graph_scroll_max(&self, rows: Rect) -> f32 {
        let rows_count = self.git.graph.commits.len() as f32;
        (rows_count * GRAPH_ROW - rows.h.max(0.0)).max(0.0)
    }

    /// 在提交图上按下第 `row` 行：开始上下拖动，内容跟着指针走。
    ///
    /// 拖动也从行上开始，按下时不打开提交；松开时移动不到 [`GRAPH_DRAG_SLOP`] 才当点击处理
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

    /// 松开提交图上的拖动：移动不到 [`GRAPH_DRAG_SLOP`] 时按点击处理，打开按下的那一行。
    ///
    /// 拖过一段距离时停在这里，不回到整行：与滚轮一样按像素滚动，行可以停在屏幕中间。
    pub(in crate::app) fn release_graph_pan(&mut self, start: (f32, f32), row: Option<usize>) {
        let (_, y) = self.pointer.mouse;
        if (y - start.1).abs() > GRAPH_DRAG_SLOP {
            return;
        }
        if let Some(row) = row {
            self.press_graph_row(row);
        }
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

/// 提交图占的高度（逻辑像素）：一行标题加八行提交。
///
/// 固定高度而不是按比例：提交信息框长高、储藏展开时改动列表照常伸缩，提交图不跟着变，
/// 列表里哪一行在什么位置也就不会跳。
const GRAPH_HEIGHT: f32 = 184.0;
/// 改动列表至少留这么高，放不下提交图时（窗口很矮）不显示提交图（逻辑像素）。
const GRAPH_MIN_ROOM: f32 = 140.0;
/// 提交图自己至少要这么高才显示（逻辑像素）：再矮就只剩标题，不如不显示。
const GRAPH_MIN: f32 = 120.0;
/// 提交图顶部标题行的高度（逻辑像素）。
pub(crate) const GRAPH_HEADER: f32 = 24.0;
/// 提交图一行的行高（逻辑像素）。
pub(crate) const GRAPH_ROW: f32 = 20.0;
/// 按下一行后移动不到这么多，松开时算点击（逻辑像素）。
const GRAPH_DRAG_SLOP: f32 = 3.0;
