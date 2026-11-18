//! 提交视图底部提交图的测试：位置、各行、点击展开提交改动的文件、滚轮滚动、拖动、获取与对比。

use super::*;

use crate::app::Drag;
use crate::app::git::graph::{GRAPH_HEADER, GRAPH_ROW, graph_rows};

#[test]
fn the_commit_graph_lists_recent_commits_below_the_changes() {
    let Some((mut app, root)) = git_app("git-commit-graph", false) else {
        return;
    };
    history_repo(&root, 40);
    app.run_action(Action::ShowCommit);
    paint(&mut app);
    let parts = app.commit_parts();
    let graph = parts.graph.expect("有提交时在改动列表下面显示提交图");
    // 上沿接着改动列表，下沿在视图底部或「上一次提交」那一栏上面。
    assert_eq!(parts.list.bottom(), graph.y);
    assert!(graph.bottom() <= app.layout.sidebar.bottom());
    let commits = app.git.graph.commits.clone();
    assert!(!commits.is_empty());
    // 一屏画得下的行都有了点击范围，从上往下就是最近的几个提交。
    let rows: Vec<usize> = app
        .hits
        .commit
        .iter()
        .filter_map(|(_, target)| match target {
            CommitTarget::GraphRow(i) => Some(*i),
            _ => None,
        })
        .collect();
    // 标题行下面每一行都算得出来：行高二十，一直排到看得到的那部分的底边。
    let shown = (graph_rows(graph).h / GRAPH_ROW).floor() as usize;
    assert_eq!(rows.len(), commits.len().min(shown));
    assert!(shown >= 3);
    assert_eq!(rows[0], 0);
    assert!(rows.windows(2).all(|pair| pair[1] == pair[0] + 1));

    // 点一行：就地展开这个提交改动的文件（文件列表随后台读取同步返回）。
    let oid = commits[0].oid.clone();
    app.press_graph_row(0);
    assert_eq!(
        app.git.graph.expanded.as_deref(),
        Some(oid.as_str()),
        "展开点中的提交"
    );
    assert!(!app.git.graph.files.is_empty(), "文件列表已经取回");
    // 再点一次：收起。
    app.press_graph_row(0);
    assert!(app.git.graph.expanded.is_none(), "收起展开的文件列表");
    assert!(app.git.graph.files.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_commit_graph_scrolls_with_the_wheel() {
    let Some((mut app, root)) = git_app("git-commit-graph-scroll", true) else {
        return;
    };
    history_repo(&root, 40);
    many_commits(&root, 24);
    app.run_action(Action::ShowCommit);
    app.git_refresh();
    paint(&mut app);
    let graph = app.commit_parts().graph.expect("显示提交图");
    let rows = graph_rows(graph);
    assert!(app.graph_scroll_max(rows) > 100.0, "提交多于一屏时能滚动");
    assert_eq!(app.git.graph.scroll, 0.0);

    app.pointer.mouse = (graph.x + 30.0, graph.y + 40.0);
    assert_eq!(
        app.scroll_target_at(graph.x + 30.0, graph.y + 40.0),
        ScrollTarget::Graph
    );
    app.on_wheel(MouseScrollDelta::LineDelta(0.0, -2.0));
    paint(&mut app);
    assert!(app.git.graph.scroll > 0.0);

    // 滚过头停在最后：最后一个提交正好挨着下沿。
    for _ in 0..40 {
        app.on_wheel(MouseScrollDelta::LineDelta(0.0, -5.0));
    }
    paint(&mut app);
    assert_eq!(app.git.graph.scroll, app.graph_scroll_max(rows));

    // 往回滚过头停在最前面。
    for _ in 0..40 {
        app.on_wheel(MouseScrollDelta::LineDelta(0.0, 5.0));
    }
    paint(&mut app);
    assert_eq!(app.git.graph.scroll, 0.0);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_graph_is_dropped_when_the_repository_switches() {
    let Some((mut app, root)) = git_app("git-commit-graph-switch", false) else {
        return;
    };
    history_repo(&root, 40);
    app.run_action(Action::ShowCommit);
    paint(&mut app);
    let old = app.git.graph.commits[0].subject.clone();

    // 打开目录下的第二个仓库：图必须换成它的提交，不能留着上一个仓库的。
    let other = root.join("nested");
    std::fs::create_dir_all(&other).unwrap();
    git_in(&other, &["init", "-q", "-b", "main"]);
    std::fs::write(other.join("a.rs"), "// a\n").unwrap();
    git_in(&other, &["add", "-A"]);
    git_in(&other, &["commit", "-q", "-m", "另一个仓库里的提交"]);
    app.git.repositories = vec![root.clone(), other.clone()];
    app.git.graph.scroll = 60.0;
    app.set_graph_compare_base(Some("origin/whatever".to_owned()));
    app.git_switch_repository(1);
    // 切换仓库时清掉旧仓库的图：读出新仓库的图之前点一行不该打开旧仓库的提交号，滚动位置和
    // 对比基准也不该跟着过来。
    assert!(
        !app.git.graph.commits.iter().any(|c| c.subject == old),
        "{:?}",
        app.git
            .graph
            .commits
            .iter()
            .map(|c| &c.subject)
            .collect::<Vec<_>>()
    );
    assert_eq!(app.git.graph.scroll, 0.0);
    assert!(app.git.graph.base.is_none());
    // 状态刷新后读的是新仓库的图。
    app.git_refresh();
    paint(&mut app);
    let commits = app.git.graph.commits.clone();
    assert_eq!(commits.len(), 1);
    assert_ne!(commits[0].subject, old);
    assert_eq!(app.git.graph.oid(0), Some(commits[0].oid.as_str()));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_graph_does_not_take_the_wheel_when_the_commit_view_is_hidden() {
    let Some((mut app, root)) = git_app("git-commit-graph-hidden", false) else {
        return;
    };
    history_repo(&root, 40);
    many_commits(&root, 24);
    app.run_action(Action::ShowCommit);
    app.git_refresh();
    paint(&mut app);
    let graph = app.commit_parts().graph.expect("显示提交图");
    let at = (graph.x + 30.0, graph.y + 40.0);
    assert_eq!(app.scroll_target_at(at.0, at.1), ScrollTarget::Graph);

    // 换成文件树：同一位置的滚轮不该去滚看不见的提交图。
    app.set_sidebar_view(SidebarView::Files);
    paint(&mut app);
    assert_ne!(app.scroll_target_at(at.0, at.1), ScrollTarget::Graph);
    app.pointer.mouse = at;
    app.on_wheel(MouseScrollDelta::LineDelta(0.0, -2.0));
    assert_eq!(app.git.graph.scroll, 0.0);
    let _ = std::fs::remove_dir_all(&root);
}

/// 图里只有当前分支（和对比的基准）的提交：别的分支独有的提交不上图。
///
/// 以前读的是全部分支，分支一多，当前分支自己的提交就被挤到一屏之外了；顺带图里全是别的分支的
/// 车道，看着也不像当前分支的历史。
#[test]
fn the_commit_graph_only_shows_the_current_branch() {
    let Some((mut app, root)) = git_app("git-commit-graph-current", false) else {
        return;
    };
    history_repo(&root, 40);
    // 另起一个分支，在它上面单独提交两个别处没有的改动。
    let before = app
        .git
        .branches
        .iter()
        .find(|b| b.current)
        .map(|b| b.name.clone());
    let branch = before.unwrap_or_else(|| "main".to_owned());
    let side = format!("side-of-{branch}");
    git_in(&root, &["switch", "-q", "-c", &side]);
    for i in 0..2 {
        std::fs::write(root.join(format!("side{i}.rs")), "// side\n").unwrap();
        git_in(&root, &["add", "-A"]);
        git_in(
            &root,
            &["commit", "-q", "-m", &format!("只在分支上的第 {i} 次改动")],
        );
    }
    git_in(&root, &["switch", "-q", &branch]);
    app.git_refresh();
    app.run_action(Action::ShowCommit);
    paint(&mut app);
    let subjects: Vec<String> = app
        .git
        .graph
        .commits
        .iter()
        .map(|c| c.subject.clone())
        .collect();
    assert!(
        !subjects.iter().any(|s| s.starts_with("只在分支上")),
        "{subjects:?}"
    );
    // 当前分支自己的提交都在（读得够深，不会因为别的分支占坑而缺一段）。
    assert!(
        subjects
            .iter()
            .any(|s| s.contains("{commit}") || !s.is_empty())
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 分支没有配置上游时，自动对比仍然拿远程上的同名分支（`origin/dev` 这样）：本地和线上差多少
/// 看得出来，不必先手动发布一次。
#[test]
fn a_branch_without_an_upstream_still_compares_with_the_remote_copy() {
    let Some((mut app, root)) = git_app("git-commit-graph-auto-base", false) else {
        return;
    };
    history_repo(&root, 40);
    let (remote, branch) = push_to_remote(&root);
    git_in(&root, &["branch", "--unset-upstream"]);
    app.run_action(Action::ShowCommit);
    app.git_refresh();
    paint(&mut app);
    let remote_copy = format!("origin/{branch}");
    assert_eq!(app.graph_upstream().as_deref(), Some(remote_copy.as_str()));
    assert_eq!(
        app.graph_compare_base().as_deref(),
        Some(remote_copy.as_str())
    );
    // 图里读的也是这个引用：基准那一侧的提交才能标出向下的箭头。
    assert_eq!(app.graph_refs(), vec![branch.clone(), remote_copy.clone()]);
    let _ = std::fs::remove_dir_all(&remote);
    let _ = std::fs::remove_dir_all(&root);
}

/// 按住提交图的上沿上下拉可以调整它占的高度：往上拉图变高、改动列表变矮，松开后记进状态文件，
/// 下次启动沿用。
#[test]
fn dragging_the_top_edge_of_the_commit_graph_resizes_it() {
    let Some((mut app, root)) = git_app("git-commit-graph-resize", false) else {
        return;
    };
    history_repo(&root, 40);
    app.run_action(Action::ShowCommit);
    paint(&mut app);
    let parts = app.commit_parts();
    let graph = parts.graph.expect("显示提交图");
    let (height, list_height) = (graph.h, parts.list.h);
    let (x, y) = (graph.x + 100.0, graph.y + 2.0);
    app.on_mouse_move(x, y);
    assert_eq!(
        app.pointer.hover,
        Hover::Commit(CommitTarget::GraphDivider),
        "上沿附近能按住"
    );
    app.on_mouse_down(MouseButton::Left);
    assert!(app.graph_resizing(), "按住上沿开始调整高度，而不是拖动内容");
    // 往上拉八十像素：图变高八十，改动列表正好矮八十，别的地方不动。
    app.on_mouse_move(x, y - 80.0);
    paint(&mut app);
    let after = app.commit_parts();
    assert_eq!(after.graph.expect("仍然显示").h, height + 80.0);
    assert_eq!(after.list.h, list_height - 80.0);
    app.on_mouse_up(MouseButton::Left);
    assert!(!app.graph_resizing());
    assert_eq!(app.state.graph_height, Some(height + 80.0), "记进状态文件");

    // 下次启动按记下的高度恢复。
    app.git.graph.height = None;
    app.restore_graph_height();
    assert_eq!(app.git.graph.height, Some(height + 80.0));

    // 往下拉过头停在最小的 {@link GRAPH_MIN}。
    let graph = app.commit_parts().graph.expect("显示提交图");
    let (x, y) = (graph.x + 100.0, graph.y + 2.0);
    app.on_mouse_move(x, y);
    app.on_mouse_down(MouseButton::Left);
    app.on_mouse_move(x, y + 900.0);
    paint(&mut app);
    assert_eq!(
        app.commit_parts().graph.expect("仍然显示").h,
        crate::app::git::graph::GRAPH_MIN
    );
    app.on_mouse_up(MouseButton::Left);
    assert!(app.state.graph_height.is_some_and(|h| h > 0.0));
    let _ = std::fs::remove_dir_all(&root);
}

/// 最后一行下面的空白处也能按住拖动：不用先找准一行才开始滚，松开时也不会打开提交。
#[test]
fn the_blank_space_under_the_commit_rows_can_be_dragged() {
    let Some((mut app, root)) = git_app("git-commit-graph-blank", false) else {
        return;
    };
    history_repo(&root, 40);
    app.run_action(Action::ShowCommit);
    paint(&mut app);
    let graph = app.commit_parts().graph.expect("显示提交图");
    let shown = (graph_rows(graph).h / GRAPH_ROW).floor() as usize;
    let rows = app.git.graph.commits.len();
    assert!(rows < shown, "提交少于一屏时下面有空白（{rows} < {shown}）");
    let (x, y) = (
        graph.x + 100.0,
        graph.y + GRAPH_HEADER + rows as f32 * GRAPH_ROW + 10.0,
    );
    app.on_mouse_move(x, y);
    assert_eq!(app.pointer.hover, Hover::Commit(CommitTarget::GraphBlank));
    app.on_mouse_down(MouseButton::Left);
    assert!(
        matches!(app.pointer.drag, Drag::GraphPan { row: None, .. }),
        "空白处按下也开始拖动"
    );
    app.on_mouse_up(MouseButton::Left);
    assert!(
        app.editor.active().and_then(|d| d.log.as_deref()).is_none(),
        "空白处松开不打开提交"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 在仓库里再提交 `count` 次：提交图上的行多于一屏，可以滚动很长一段。
fn many_commits(root: &Path, count: usize) {
    for i in 0..count {
        std::fs::write(root.join(format!("src/more{i}.rs")), format!("// {i}\n")).unwrap();
        git_in(root, &["add", "-A"]);
        let message = format!("第 {i} 次改动：细小的调整");
        git_in(root, &["commit", "-q", "-m", &message]);
    }
}

#[test]
fn the_commit_graph_follows_the_pointer_while_dragged() {
    let Some((mut app, root)) = git_app("git-commit-graph-drag", true) else {
        return;
    };
    history_repo(&root, 40);
    many_commits(&root, 24);
    app.run_action(Action::ShowCommit);
    app.git_refresh();
    paint(&mut app);
    let graph = app.commit_parts().graph.expect("显示提交图");
    let rows = graph_rows(graph);
    assert!(app.graph_scroll_max(rows) > 100.0, "提交多于一屏时能滚动");
    // 按住第三行往上拖五十像素：内容跟着指针走，松开时算拖动，不打开提交。
    let (x, y) = (graph.x + 40.0, rows.y + 2.5 * GRAPH_ROW);
    app.pointer.mouse = (x, y);
    app.on_mouse_down(MouseButton::Left);
    assert!(
        matches!(app.pointer.drag, Drag::GraphPan { row: Some(2), .. }),
        "按下图上的一行开始拖动"
    );
    app.on_mouse_move(x, y - 50.0);
    paint(&mut app);
    assert_eq!(app.git.graph.scroll, 50.0);
    assert_eq!(app.pointer.hover, Hover::None, "拖动时行不高亮");
    app.on_mouse_up(MouseButton::Left);
    assert!(
        app.editor.active().and_then(|d| d.log.as_deref()).is_none(),
        "拖动过就不打开提交"
    );

    // 往下拖回去，拖过头停在最前面。
    app.pointer.mouse = (x, y);
    app.on_mouse_down(MouseButton::Left);
    app.on_mouse_move(x, y + 120.0);
    paint(&mut app);
    assert_eq!(app.git.graph.scroll, 0.0);
    app.on_mouse_up(MouseButton::Left);

    // 往上拖过头停在最后一行贴着下沿。
    app.pointer.mouse = (x, y);
    app.on_mouse_down(MouseButton::Left);
    app.on_mouse_move(x, y - 900.0);
    paint(&mut app);
    assert_eq!(app.git.graph.scroll, app.graph_scroll_max(rows));
    app.on_mouse_up(MouseButton::Left);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_click_without_moving_expands_the_commit() {
    let Some((mut app, root)) = git_app("git-commit-graph-click", false) else {
        return;
    };
    history_repo(&root, 40);
    app.run_action(Action::ShowCommit);
    paint(&mut app);
    let graph = app.commit_parts().graph.expect("显示提交图");
    let rows = graph_rows(graph);
    let commits = app.git.graph.commits.clone();
    let oid = commits[2].oid.clone();
    // 按下再松开、中间没有移动：展开按下的那一行。
    let (x, y) = (graph.x + 40.0, rows.y + 2.5 * GRAPH_ROW);
    app.pointer.mouse = (x, y);
    app.on_mouse_down(MouseButton::Left);
    assert!(app.git.graph.expanded.is_none(), "按下时不展开");
    app.on_mouse_up(MouseButton::Left);
    assert_eq!(
        app.git.graph.expanded.as_deref(),
        Some(oid.as_str()),
        "松开时展开按下的那一行"
    );
    assert!(!app.git.graph.files.is_empty(), "文件列表已经取回");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_commit_graph_scrollbar_can_be_dragged() {
    let Some((mut app, root)) = git_app("git-commit-graph-bar", true) else {
        return;
    };
    history_repo(&root, 40);
    many_commits(&root, 24);
    app.run_action(Action::ShowCommit);
    app.git_refresh();
    paint(&mut app);
    let graph = app.commit_parts().graph.expect("显示提交图");
    let rows = graph_rows(graph);
    let max = app.graph_scroll_max(rows);
    assert!(max > 100.0);
    let thumb = scrollbar_thumb(rows, 0.0, max + rows.h).expect("能滚动时有滑块");
    // 鼠标移到行区域右端：滚动条优先于它下面的行。
    let (x, y) = (rows.right() - 4.0, thumb.y + thumb.h / 2.0);
    app.on_mouse_move(x, y);
    assert_eq!(app.pointer.hover, Hover::Scrollbar(Scrollbar::Graph));

    // 按住滑块往下拖：内容跟着走。
    app.on_mouse_down(MouseButton::Left);
    assert!(matches!(
        app.pointer.drag,
        Drag::Scrollbar {
            bar: Scrollbar::Graph,
            ..
        }
    ));
    app.on_mouse_move(x, rows.bottom() - 10.0);
    paint(&mut app);
    assert!(app.git.graph.scroll > 0.0);
    assert_eq!(
        app.scroll_target_at(x, y),
        ScrollTarget::Graph,
        "拖着滑块时滚轮仍然滚动提交图"
    );
    app.on_mouse_up(MouseButton::Left);
    // 点轨道上滑块以外的位置时滑块跳到鼠标处。
    app.pointer.mouse = (x, rows.y + 2.0);
    app.on_mouse_down(MouseButton::Left);
    app.on_mouse_up(MouseButton::Left);
    assert_eq!(app.git.graph.scroll, 0.0);
    let _ = std::fs::remove_dir_all(&root);
}

/// 建一个真正的远程仓库并把当前分支推上去，返回远程目录和分支的名字：之后获取、拉取、推送
/// 都有上游可用。
fn push_to_remote(root: &Path) -> (PathBuf, String) {
    let remote = root.with_extension("remote.git");
    let _ = std::fs::remove_dir_all(&remote);
    git_in(
        root.parent().unwrap(),
        &["init", "-q", "--bare", remote.to_str().unwrap()],
    );
    let branch = String::from_utf8(
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned();
    // 远程仓库的 HEAD 还指着别的名字：推上去以后把它指到这条分支，克隆时才会检出它。
    git_in(root, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git_in(root, &["push", "-q", "-u", "origin", "HEAD"]);
    git_in(
        &remote,
        &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")],
    );
    (remote, branch)
}

/// 标题行右端的三个按钮（获取、拉取、推送）与对比控件占的位置。
fn graph_tools(app: &App) -> [(Rect, CommitTarget); 4] {
    let find = |target: CommitTarget| {
        app.hits
            .commit
            .iter()
            .find(|(_, t)| *t == target)
            .map(|(rect, _)| *rect)
            .unwrap_or_else(|| panic!("标题行上画了 {target:?}"))
    };
    [
        (find(CommitTarget::GraphFetch), CommitTarget::GraphFetch),
        (find(CommitTarget::GraphPull), CommitTarget::GraphPull),
        (find(CommitTarget::GraphPush), CommitTarget::GraphPush),
        (find(CommitTarget::GraphCompare), CommitTarget::GraphCompare),
    ]
}

#[test]
fn the_graph_compares_the_branch_with_its_upstream() {
    let Some((mut app, root)) = git_app("git-commit-graph-compare", false) else {
        return;
    };
    history_repo(&root, 40);
    // 推到远程以后再走两个提交：当前分支领先上游两个。
    let (remote, _) = push_to_remote(&root);
    many_commits(&root, 2);
    app.run_action(Action::ShowCommit);
    app.git_refresh();
    paint(&mut app);
    let upstream = app.graph_upstream().expect("推到远程后当前分支有上游");
    let compare = app.git.graph.compare.as_ref().expect("自动与上游对比");
    assert_eq!(compare.base, upstream);
    assert_eq!(compare.counts, (2, 0));
    // 只有本地那两个提交算领先，更早的已经在上游里。
    let commits = app.git.graph.commits.clone();
    assert_eq!(compare.side(&commits[0].oid), Some(true));
    assert_eq!(compare.side(&commits[1].oid), Some(true));
    assert_eq!(compare.side(&commits[2].oid), None);

    // 标题行右端：三个按钮在右边，对比控件在它们左边，都在标题行里。
    let graph = app.commit_parts().graph.expect("显示提交图");
    let tools = graph_tools(&app);
    let (fetch, pull, push, chip) = (tools[0].0, tools[1].0, tools[2].0, tools[3].0);
    for rect in [fetch, pull, push, chip] {
        assert!(rect.y >= graph.y, "在标题行里");
        assert!(rect.bottom() <= graph.y + GRAPH_HEADER, "不压到第一行提交");
        assert!(rect.right() <= graph.right(), "在侧栏里");
    }
    assert!(
        fetch.right() < pull.x && pull.right() < push.x,
        "从左到右获取、拉取、推送"
    );
    assert!(chip.right() < fetch.x, "对比控件在按钮左边");
    assert!(chip.x > graph.x + 40.0, "标题不压到对比控件");

    // 换基准：与五个提交前比，领先五个；再回到自动。
    app.set_graph_compare_base(Some("HEAD~5".into()));
    paint(&mut app);
    let compare = app.git.graph.compare.as_ref().expect("换基准后重新对比");
    let (ahead, behind) = compare.counts;
    assert_eq!((compare.base.as_str(), behind), ("HEAD~5", 0));
    assert!(ahead >= 5, "五个提交以前作基准时领先至少五个：{ahead}");
    app.set_graph_compare_base(None);
    paint(&mut app);
    let compare = app.git.graph.compare.as_ref().expect("回到自动对比上游");
    assert_eq!(compare.base, upstream);
    assert_eq!(compare.counts, (2, 0));

    // 在日志标签里比较当前分支与基准：打开该页，基准写在上游一侧。
    app.graph_compare_in_log();
    paint(&mut app);
    let view = app.editor.active().and_then(|d| d.log.as_deref());
    let view = view.expect("打开比较页");
    let compare = view.compare.as_ref().expect("比较当前分支与基准");
    let current = app.git.branches.iter().find(|b| b.current).unwrap();
    assert_eq!(
        (compare.from.as_str(), compare.to.as_str()),
        (current.name.as_str(), upstream.as_str())
    );
    let _ = std::fs::remove_dir_all(&remote);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_graph_marks_the_commits_only_one_side_has() {
    let Some((mut app, root)) = git_app("git-commit-graph-behind", false) else {
        return;
    };
    history_repo(&root, 40);
    let (remote, branch) = push_to_remote(&root);
    // 别处（另一个克隆）往远程推一个提交，本地获取以后落后一个、不领先。
    let other = root.with_extension("other");
    let _ = std::fs::remove_dir_all(&other);
    git_in(
        root.parent().unwrap(),
        &[
            "clone",
            "-q",
            remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join("theirs.rs"), "// theirs\n").unwrap();
    git_in(&other, &["add", "-A"]);
    git_in(&other, &["commit", "-q", "-m", "别处的改动"]);
    git_in(
        &other,
        &["push", "-q", "origin", &format!("HEAD:refs/heads/{branch}")],
    );
    many_commits(&root, 1);
    app.run_action(Action::ShowCommit);
    app.git_refresh();
    app.run_branch_command(MenuCommand::GitFetch);
    app.git_refresh();
    paint(&mut app);
    let compare = app.git.graph.compare.as_ref().expect("自动与上游对比");
    assert_eq!(compare.counts, (1, 1), "本地一个提交没推，上游有一个没拉");
    let commits = app.git.graph.commits.clone();
    let ahead = commits
        .iter()
        .position(|c| Some(true) == compare.side(&c.oid));
    let behind = commits
        .iter()
        .position(|c| Some(false) == compare.side(&c.oid));
    assert!(ahead.is_some() && behind.is_some());
    let _ = std::fs::remove_dir_all(&other);
    let _ = std::fs::remove_dir_all(&remote);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_graph_buttons_run_fetch_pull_and_push() {
    let Some((mut app, root)) = git_app("git-commit-graph-tools", false) else {
        return;
    };
    history_repo(&root, 40);
    app.run_action(Action::ShowCommit);
    app.git_refresh();
    paint(&mut app);
    // 没有远程仓库也没有上游：不画领先、落后的个数，但对比的入口还在。
    assert!(app.graph_compare_base().is_none());
    assert!(app.git.graph.compare.is_none());
    let fetch_tip = app.graph_tip(CommitTarget::GraphFetch);
    assert!(fetch_tip.contains("远程"), "{fetch_tip}");
    let compare_tip = app.graph_tip(CommitTarget::GraphCompare);
    assert!(
        !compare_tip.is_empty() && compare_tip != fetch_tip,
        "{compare_tip}"
    );
    let graph = app.commit_parts().graph.expect("显示提交图");
    let tools = graph_tools(&app);
    assert!(tools.iter().all(|(rect, _)| rect.y >= graph.y));

    // 点推送：与命令面板、分支菜单走同一个入口，没有远程时说明原因。
    app.press_commit(CommitTarget::GraphPush);
    assert!(
        app.notice
            .text
            .as_deref()
            .is_some_and(|n| n.contains("未配置远程仓库")),
        "{:?}",
        app.notice.text
    );

    // 点对比控件弹出换基准的菜单：没有分支时只剩「在日志里比较」。
    app.press_commit(CommitTarget::GraphCompare);
    assert_eq!(
        app.overlays.context_menu.as_ref().map(|m| m.target.clone()),
        Some(MenuTarget::GraphCompare)
    );
    let commands = menu_commands(&app);
    let enabled = |command: MenuCommand| {
        commands
            .iter()
            .find(|(c, _)| *c == command)
            .is_some_and(|(_, on)| *on)
    };
    assert!(enabled(MenuCommand::GraphCompareLog));
    // 没有上游时「自动」不能用，但仍可自己挑一个基准（本地分支）。
    assert!(!enabled(MenuCommand::GraphCompareAuto));
    let bases: Vec<usize> = commands
        .iter()
        .filter_map(|(c, on)| match c {
            MenuCommand::GraphCompareBase(i) if *on => Some(*i),
            _ => None,
        })
        .collect();
    assert_eq!(
        bases.len(),
        app.git.branches.iter().filter(|b| !b.current).count(),
        "当前分支以外的分支都能当基准"
    );
    app.dismiss_popup();

    // 加上远程：拉取、推送有上游可用，按钮的提示写明上游。
    let (remote, _) = push_to_remote(&root);
    app.git_refresh();
    paint(&mut app);
    let upstream = app.graph_upstream().expect("有上游");
    for target in [CommitTarget::GraphPull, CommitTarget::GraphPush] {
        let tip = app.graph_tip(target);
        assert!(tip.contains(&upstream), "{target:?} 的提示写明上游：{tip}");
    }
    assert!(
        app.graph_tip(CommitTarget::GraphCompare)
            .contains(&upstream),
        "对比控件的提示写明基准与领先、落后的个数"
    );
    let _ = std::fs::remove_dir_all(&remote);
    let _ = std::fs::remove_dir_all(&root);
}

/// 展开的提交：文件就地列在这一行下面，点一个文件打开它的当前内容。
#[test]
fn the_expanded_files_open_in_the_editor() {
    let Some((mut app, root)) = git_app("git-commit-graph-files", false) else {
        return;
    };
    history_repo(&root, 40);
    many_commits(&root, 24);
    app.run_action(Action::ShowCommit);
    app.git_refresh();
    paint(&mut app);
    // 展开最近的一个提交：它改动的文件列在下面，各有一个命中范围。
    app.press_graph_row(0);
    paint(&mut app);
    let files = app.git.graph.files.clone();
    assert!(!files.is_empty(), "文件列表取回来了");
    let hits: Vec<(Rect, CommitTarget)> = app
        .hits
        .commit
        .iter()
        .filter_map(|(r, t)| matches!(t, CommitTarget::GraphFile(_)).then_some((*r, *t)))
        .collect();
    assert_eq!(hits.len(), files.len(), "每个文件一行，都点得到");
    // 展开的列表计入滚动范围：滚得比不展开时更远。
    let graph = app.commit_parts().graph.expect("显示提交图");
    let rows = graph_rows(graph);
    let expanded = app.graph_scroll_max(rows);
    app.collapse_graph_files();
    assert!(
        expanded > app.graph_scroll_max(rows),
        "展开时滚得更远：{expanded}"
    );
    // 点第一个文件：编辑区打开它的当前内容。
    let first = app.git.graph.commits[0].oid.clone();
    app.expand_graph_commit(&first);
    paint(&mut app);
    let first = files[0].path.clone();
    app.press_commit(CommitTarget::GraphFile(0));
    let opened = app
        .editor
        .active()
        .and_then(|d| d.path.clone())
        .expect("点开的文件在编辑区里打开");
    assert_eq!(opened, first);
    let _ = std::fs::remove_dir_all(&root);
}

/// 提交图里提交的右键菜单：展开文件、到日志中查看，以及与日志菜单共用的一套
/// （拣选、复制提交号与说明……）。
#[test]
fn the_graph_right_click_menu_offers_the_log_actions() {
    let Some((mut app, root)) = git_app("git-commit-graph-menu", false) else {
        return;
    };
    history_repo(&root, 40);
    app.run_action(Action::ShowCommit);
    paint(&mut app);
    let oid = app.git.graph.commits[1].oid.clone();
    app.open_context_menu(MenuTarget::GraphCommit(oid.clone()), (0.0, 0.0));
    let commands = menu_commands(&app);
    let has = |commands: &[(MenuCommand, bool)], command: MenuCommand| {
        commands.iter().any(|(c, _)| *c == command)
    };
    assert!(has(&commands, MenuCommand::GraphShowInLog));
    assert!(has(&commands, MenuCommand::GraphOpenFiles));
    // 与日志的提交菜单同一套命令。
    assert!(has(&commands, MenuCommand::LogCherryPick));
    assert!(has(&commands, MenuCommand::LogCopyOid));
    assert!(has(&commands, MenuCommand::LogCopyShortOid));
    assert!(has(&commands, MenuCommand::LogCopyMessage));
    assert!(has(&commands, MenuCommand::LogRevert));
    app.dismiss_popup();
    // 复制提交号：剪贴板里是完整的提交号。
    app.open_context_menu(MenuTarget::GraphCommit(oid.clone()), (0.0, 0.0));
    app.run_menu_command(MenuCommand::LogCopyOid);
    assert_eq!(
        app.clipboard.copied.as_deref(),
        Some(oid.as_str()),
        "复制了完整的提交号"
    );
    app.dismiss_popup();
    // 菜单里点「展开改动的文件」：与点击这一行一样展开。
    app.open_context_menu(MenuTarget::GraphCommit(oid.clone()), (0.0, 0.0));
    app.run_menu_command(MenuCommand::GraphOpenFiles);
    assert_eq!(app.git.graph.expanded.as_deref(), Some(oid.as_str()));
    app.dismiss_popup();
    // 再开一次菜单：已展开的提交写的是「收起文件列表」。
    app.open_context_menu(MenuTarget::GraphCommit(oid), (0.0, 0.0));
    let commands = menu_commands(&app);
    assert!(has(&commands, MenuCommand::GraphHideFiles));
    app.dismiss_popup();
    let _ = std::fs::remove_dir_all(&root);
}

/// 画出具名仓库的提交视图，供人工查看提交图（改动列表多、储藏、上一次提交都在）。
#[test]
#[ignore = "截图，手动运行"]
fn shoot_commit_graph() {
    for dark in [false, true] {
        let theme = if dark { "dark" } else { "light" };
        let dir = search_folder(&format!("commit-graph-{theme}"));
        if !long_repo(&dir) {
            return;
        }
        history_repo(&dir, 40);
        many_commits(&dir, 24);
        // 与上游有来有往：本地两个提交没推，远程有一个没拉，标题上写「↑2 ↓1」，两边的提交行
        // 前各有一个箭头。
        let (remote, branch) = push_to_remote(&dir);
        for i in 0..2 {
            std::fs::write(dir.join(format!("ahead{i}.rs")), format!("// ahead {i}\n")).unwrap();
            git_in(&dir, &["add", "-A"]);
            git_in(&dir, &["commit", "-q", "-m", &format!("本地第 {i} 个改动")]);
        }
        let other = dir.with_extension("other");
        let _ = std::fs::remove_dir_all(&other);
        git_in(
            dir.parent().unwrap(),
            &[
                "clone",
                "-q",
                remote.to_str().unwrap(),
                other.to_str().unwrap(),
            ],
        );
        std::fs::write(other.join("theirs.rs"), "// theirs\n").unwrap();
        git_in(&other, &["add", "-A"]);
        git_in(&other, &["commit", "-q", "-m", "远程上的改动"]);
        git_in(
            &other,
            &["push", "-q", "origin", &format!("HEAD:refs/heads/{branch}")],
        );
        git_in(&dir, &["fetch", "-q", "origin"]);
        std::fs::write(dir.join("notes.md"), "# notes\n").unwrap();
        let mut app = app_with(&dir, dark);
        app.settings_toggle(Setting::GitModule);
        app.run_action(Action::ShowCommit);
        paint(&mut app);
        // 滚动一段并停一会儿：滚动条淡入之后才画得出来，截图里能看到。
        app.git.graph.scroll = 20.0;
        for _ in 0..12 {
            paint(&mut app);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        shoot(&mut app, &format!("commit-graph-{theme}"));
        let _ = std::fs::remove_dir_all(&other);
        let _ = std::fs::remove_dir_all(&remote);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
