//! Git 日志的日期、路径筛选，详情里的所在分支、按目录分组的文件，以及左侧的分支面板。

use super::*;
use crate::app::git::log::{BranchRow, LogTarget};
use crate::app::input::context_menu::{MenuCommand, MenuTarget};

/// 在仓库 `root` 里数 `git log --all` 加上 `args` 列出的提交。
fn count(root: &Path, args: &[&str]) -> usize {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["log", "--all", "--exclude=refs/stash", "--oneline"])
        .args(args)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).lines().count()
}

#[test]
fn log_filters_by_date_and_path_and_lists_the_containing_branches() {
    let Some((mut app, root)) = git_app("git-log-more-filters", false) else {
        return;
    };
    history_repo(&root, 10);
    app.git_refresh();
    show_all_log(&mut app);
    paint(&mut app);
    let listed = |app: &App| {
        app.editor
            .active()
            .unwrap()
            .log
            .as_deref()
            .unwrap()
            .commits
            .len()
    };
    let all = listed(&app);
    assert_eq!(all, count(&root, &[]));

    // 日期：最近 7 天，与 `git log --since` 一致；改回全部日期。测试仓库每天一个提交，正好有
    // 一个在「7 天前」的分界上，筛选前后各数一次，和其中之一相同即可。
    let before = count(&root, &["--since=7.days.ago"]);
    app.press_log(LogTarget::Date);
    app.run_menu_command(MenuCommand::LogSince(7));
    let after = count(&root, &["--since=7.days.ago"]);
    assert!(after > 0 && before < all);
    assert!([before, after].contains(&listed(&app)));
    app.open_context_menu(MenuTarget::LogDate, (0.0, 0.0));
    app.run_menu_command(MenuCommand::LogSince(0));
    assert_eq!(listed(&app), all);

    // 路径：菜单里列出仓库根目录下的目录；输入的路径也算上，再选一次去掉。
    paint(&mut app);
    app.press_log(LogTarget::Path);
    assert!(app.git.log.paths.iter().any(|p| p == "src"));
    app.overlays.context_menu = None;
    app.toggle_log_path("src/login.rs".into());
    assert_eq!(listed(&app), count(&root, &["--", "src/login.rs"]));
    assert!(listed(&app) < all);
    app.toggle_log_path("src/login.rs".into());
    assert_eq!(listed(&app), all);

    // 所在分支：选中 `HEAD` 以后停一会儿才查（连续移动时不查），查到的含当前分支 `main`。
    let head = app
        .editor
        .active()
        .unwrap()
        .log
        .as_deref()
        .unwrap()
        .head_row();
    app.select_log_row(head.unwrap());
    let branches = |app: &App| {
        let view = app.editor.active().unwrap().log.as_deref().unwrap();
        view.details.as_ref().and_then(|d| d.branches.clone())
    };
    assert!(branches(&app).is_none());
    app.log_tick(std::time::Instant::now() + std::time::Duration::from_secs(1));
    app.poll_log();
    assert!(branches(&app).is_some_and(|b| b.iter().any(|n| n == "main")));
}

/// 分支面板里名为 `name` 的分支在第几行；它所在的组、目录收着时依次展开。
fn panel_row(app: &mut App, name: &str) -> usize {
    loop {
        let rows = app.log_branch_rows();
        let found = rows.iter().position(|r| {
            matches!(r, BranchRow::Branch { index, .. } if app.git.branches[*index].name == name)
        });
        if let Some(i) = found {
            return i;
        }
        let closed = rows.iter().position(|r| {
            matches!(
                r,
                BranchRow::Remotes { open: false, .. } | BranchRow::Folder { open: false, .. }
            )
        });
        app.press_log(LogTarget::BranchRow(closed.expect(name)));
    }
}

#[test]
fn the_branch_panel_lists_branches_like_the_picker_and_filters_by_one() {
    let Some((mut app, root)) = git_app("git-log-branch-panel", false) else {
        return;
    };
    history_repo(&root, 10);
    let long = "release/a-very-long-branch-name-that-does-not-fit";
    git_in(&root, &["branch", long]);
    // 第二个远程：远程分支一组里按远程名分出目录。
    git_in(
        &root,
        &["remote", "add", "upstream", "https://example.invalid/r.git"],
    );
    git_in(&root, &["update-ref", "refs/remotes/upstream/main", "main"]);
    app.git_refresh();
    show_all_log(&mut app);
    app.layout.editor.w = app.layout.editor.w.max(1200.0);
    paint(&mut app);
    assert!(app.log_parts().branches.w > 0.0);

    // 「全部分支」在最上面，之后与分支选择器相同：本地一组，当前分支在最前。
    let rows = app.log_branch_rows();
    assert_eq!(rows[0], BranchRow::All);
    assert!(matches!(
        rows[1],
        BranchRow::Locals {
            recent: false,
            open: true,
            ..
        }
    ));
    assert!(
        matches!(&rows[2], BranchRow::Branch { index, .. } if app.git.branches[*index].current)
    );

    // 远程分支一组平时收起，展开后每个远程一个目录，目录里写去掉远程名的名字。
    let upstream = panel_row(&mut app, "upstream/main");
    let rows = app.log_branch_rows();
    assert!(matches!(&rows[upstream], BranchRow::Branch { label, .. } if label == "main"));
    for remote in ["origin", "upstream"] {
        let folder =
            |r: &BranchRow| matches!(r, BranchRow::Folder { label, .. } if label == remote);
        assert!(rows.iter().any(folder));
    }

    // 放不下的分支名截短，悬停时提示全名。
    let cut = panel_row(&mut app, long);
    paint(&mut app);
    assert!(app.git.log.long_branches.contains(&cut));
    let dev = panel_row(&mut app, "dev");
    assert!(!app.git.log.long_branches.contains(&dev));

    // 点一个分支只看它的提交，再点一次回到全部。
    let login = panel_row(&mut app, "feature/login");
    app.press_log(LogTarget::BranchRow(login));
    assert_eq!(app.log_branch_filter(), Some("feature/login"));
    app.press_log(LogTarget::BranchRow(login));
    assert_eq!(app.log_branch_filter(), None);

    // 右键是分支选择器里同样的操作菜单；双击签出。
    paint(&mut app);
    let hit = app
        .hits
        .log
        .iter()
        .find(|(_, t)| *t == LogTarget::BranchRow(login));
    let (rect, _) = *hit.unwrap();
    let index = app
        .log_branch_context_target(rect.x + 20.0, rect.y + 4.0)
        .unwrap();
    assert_eq!(app.git.branches[index].name, "feature/login");
    app.checkout_log_branch_row(login);
    app.git_refresh();
    assert!(
        app.git
            .branches
            .iter()
            .any(|b| b.current && b.name == "feature/login")
    );

    // 收起本地分支一组；隐藏面板后列表占回它的宽度。
    let locals = (app.log_branch_rows().iter())
        .position(|r| matches!(r, BranchRow::Locals { recent: false, .. }))
        .unwrap();
    app.press_log(LogTarget::BranchRow(locals));
    let named = |app: &App, name: &str| {
        (app.log_branch_rows().iter()).any(|r| {
            matches!(r, BranchRow::Branch { index, .. } if app.git.branches[*index].name == name)
        })
    };
    assert!(!named(&app, "dev"));
    app.press_log(LogTarget::BranchRow(locals));
    let list = app.log_parts().list.x;
    app.press_log(LogTarget::BranchesToggle);
    assert_eq!(app.log_parts().branches.w, 0.0);
    assert!(app.log_parts().list.x < list);
}

#[test]
fn log_menus_filter_by_me_copy_a_patch_and_compare_a_file_with_local() {
    let Some((mut app, root)) = git_app("git-log-more-menus", false) else {
        return;
    };
    history_repo(&root, 10);
    git_in(&root, &["config", "user.email", "alice@example.com"]);
    app.git_refresh();
    show_all_log(&mut app);
    paint(&mut app);
    let view = |app: &App| {
        app.editor
            .active()
            .unwrap()
            .log
            .as_deref()
            .unwrap()
            .commits
            .clone()
    };

    // 作者菜单里「我」按仓库配置的邮箱筛选。
    assert_eq!(app.git.my_email, "alice@example.com");
    app.press_log(LogTarget::Author);
    app.run_menu_command(MenuCommand::LogAuthorMe);
    let mine = view(&app);
    assert!(!mine.is_empty() && mine.iter().all(|c| c.email == "alice@example.com"));
    app.press_log(LogTarget::ClearFilters);

    // 复制为补丁：`git format-patch` 的格式，以该提交号开头（中文说明在邮件头里会编码）。
    let commit = view(&app)[0].clone();
    app.open_context_menu(MenuTarget::LogCommit(commit.oid.clone()), (0.0, 0.0));
    app.run_menu_command(MenuCommand::LogCopyPatch);
    app.poll_log();
    let patch = app.clipboard.copied.clone().unwrap_or_default();
    assert!(
        patch.starts_with(&format!("From {} ", commit.oid)),
        "{patch}"
    );

    // 合并提交没有补丁：不复制空的内容，剪贴板里还是上一个补丁。
    let merge = view(&app)
        .into_iter()
        .find(|c| c.parents.len() > 1)
        .unwrap();
    app.open_context_menu(MenuTarget::LogCommit(merge.oid.clone()), (0.0, 0.0));
    app.run_menu_command(MenuCommand::LogCopyPatch);
    app.poll_log();
    assert_eq!(app.clipboard.copied.as_deref(), Some(patch.as_str()));

    // 与本地版本比较：左边是该提交里的版本，右边是工作区里的。
    let path = root.join("src/login.rs");
    std::fs::write(&path, "local edit\n").unwrap();
    let row = view(&app)
        .iter()
        .position(|c| c.subject == "登录：失败三次后锁定")
        .unwrap();
    app.select_log_row(row);
    app.open_context_menu(MenuTarget::LogFile(0, path.clone()), (0.0, 0.0));
    app.run_menu_command(MenuCommand::LogCompareWithLocal);
    app.poll_log();
    let doc = app.editor.active().unwrap();
    assert!(
        doc.title
            .as_deref()
            .is_some_and(|t| t.starts_with("login.rs"))
    );
    let diff = doc.diff.as_deref().unwrap();
    assert_eq!(diff.new.text.to_string(), "local edit\n");
}

#[test]
fn log_filters_pick_several_branches_and_authors() {
    let Some((mut app, root)) = git_app("git-log-multi-filters", false) else {
        return;
    };
    history_repo(&root, 10);
    app.git_refresh();
    show_all_log(&mut app);
    app.layout.editor.w = app.layout.editor.w.max(1200.0);
    paint(&mut app);
    let query = |app: &App| {
        app.editor
            .active()
            .unwrap()
            .log
            .as_deref()
            .unwrap()
            .query
            .clone()
    };
    let listed = |app: &App| {
        app.editor
            .active()
            .unwrap()
            .log
            .as_deref()
            .unwrap()
            .commits
            .len()
    };
    let at = |app: &mut App, name: &str| panel_row(app, name);

    // 分支面板里按住 ⌘ 点：在已选的分支里加上，两个分支的提交都列出；再按住 ⌘ 点去掉。
    let row = at(&mut app, "dev");
    app.press_log(LogTarget::BranchRow(row));
    app.pointer.modifiers = command_modifiers();
    let row = at(&mut app, "feature/login");
    app.press_log(LogTarget::BranchRow(row));
    assert_eq!(query(&app).branches, ["dev", "feature/login"]);
    let out = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-list", "--count", "dev", "feature/login"])
        .output()
        .unwrap();
    let both: usize = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
    assert_eq!(listed(&app), both);
    assert!(!query(&app).graphless());
    let row = at(&mut app, "feature/login");
    app.press_log(LogTarget::BranchRow(row));
    assert_eq!(query(&app).branches, ["dev"]);
    app.pointer.modifiers = winit::keyboard::ModifiersState::empty();
    app.press_log(LogTarget::BranchRow(0));
    assert!(query(&app).branches.is_empty());

    // 作者菜单里选两位：两位的提交都列出；「全部作者」清空。
    paint(&mut app);
    app.press_log(LogTarget::Author);
    let authors = app.git.log.authors.clone();
    assert!(authors.len() >= 2);
    app.run_menu_command(MenuCommand::LogAuthor(1));
    app.press_log(LogTarget::Author);
    app.run_menu_command(MenuCommand::LogAuthor(2));
    assert_eq!(query(&app).authors, authors[..2]);
    let view = app.editor.active().unwrap().log.as_deref().unwrap();
    assert!(
        view.commits
            .iter()
            .all(|c| authors[..2].contains(&c.author))
    );
    app.press_log(LogTarget::Author);
    app.run_menu_command(MenuCommand::LogAuthor(0));
    assert!(query(&app).authors.is_empty());
}

#[test]
fn a_path_filtered_log_keeps_its_graph_and_real_parents() {
    let Some((mut app, root)) = git_app("git-log-path-graph", false) else {
        return;
    };
    history_repo(&root, 10);
    app.git_refresh();
    show_all_log(&mut app);
    app.toggle_log_path("src/login.rs".into());
    paint(&mut app);
    let view = app.editor.active().unwrap().log.as_deref().unwrap();
    // 按路径筛选照样画提交图：父提交改写成往下最近一个也改过这个文件的提交，都在列表里。
    assert!(!view.query.graphless());
    let oids: Vec<String> = view.commits.iter().map(|c| c.oid.clone()).collect();
    let newest = view.commits[0].clone();
    assert!(newest.parents.iter().all(|p| oids.contains(p)));

    // 详情里的文件和真正的第一个父提交比，只是这个提交自己的改动。
    app.select_log_row(0);
    let out = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", &format!("{}^1", newest.oid)])
        .output()
        .unwrap();
    let real = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    let view = app.editor.active().unwrap().log.as_deref().unwrap();
    let details = view.details.as_ref().unwrap();
    assert_eq!(details.base.as_deref(), Some(real.as_str()));
    let files = details.files.as_ref().unwrap();
    assert!(files.iter().any(|f| f.path.ends_with("src/login.rs")));
}

#[test]
fn log_columns_can_be_hidden_shown_and_widened() {
    use crate::app::git::log::{AUTHOR_DEFAULT, LogColumn};
    let Some((mut app, root)) = git_app("git-log-columns", false) else {
        return;
    };
    history_repo(&root, 10);
    app.git_refresh();
    show_all_log(&mut app);
    app.layout.editor.w = app.layout.editor.w.max(1200.0);
    paint(&mut app);
    let has = |app: &App, f: &dyn Fn(&LogTarget) -> bool| app.hits.log.iter().any(|(_, t)| f(t));

    // 默认显示作者和时间，不显示提交号；「⋯」里勾上提交号、去掉作者。
    assert!(app.log_column_shown(LogColumn::Author) && !app.log_column_shown(LogColumn::Hash));
    app.press_log(LogTarget::Columns);
    app.run_menu_command(MenuCommand::LogColumn(LogColumn::Hash));
    assert!(app.log_column_shown(LogColumn::Hash));
    assert_eq!(
        app.state.log_columns.as_deref(),
        Some(&["author".into(), "date".into(), "hash".into()][..])
    );
    app.press_log(LogTarget::Columns);
    app.run_menu_command(MenuCommand::LogColumn(LogColumn::Author));
    paint(&mut app);
    assert!(!has(&app, &|t| matches!(
        t,
        LogTarget::Avatar(_) | LogTarget::AuthorEdge
    )));

    // 作者一列拖宽：往左拖变宽，最宽有限度；双击恢复默认宽度。
    app.press_log(LogTarget::Columns);
    app.run_menu_command(MenuCommand::LogColumn(LogColumn::Author));
    paint(&mut app);
    let (edge, _) = *app
        .hits
        .log
        .iter()
        .find(|(_, t)| *t == LogTarget::AuthorEdge)
        .unwrap();
    app.press_log_author_edge(edge.x, false);
    // 拖过以后按拖的宽度显示名字，不再随列表变窄收成只剩头像。
    assert_eq!(app.log_author_show(1.0), 0.0);
    app.drag_log_author_edge(edge.x, (edge.x, AUTHOR_DEFAULT));
    assert_eq!(app.log_author_show(1.0), 1.0);
    app.drag_log_author_edge(edge.x - 40.0, (edge.x, AUTHOR_DEFAULT));
    assert_eq!(app.git.log.author_w, Some(AUTHOR_DEFAULT + 40.0));
    app.drag_log_author_edge(edge.x - 1000.0, (edge.x, AUTHOR_DEFAULT));
    assert!(app.git.log.author_w.unwrap() < AUTHOR_DEFAULT * 3.0);
    app.press_log_author_edge(edge.x, true);
    assert_eq!(app.git.log.author_w, None);
    let _ = std::fs::remove_dir_all(&root);
}

/// 「作者」下拉可以键入文字筛选：名单不受当前筛选条件影响（日期、路径收窄了取回的提交，作者仍然
/// 全列出来），键入几位只剩名字里含这几个字的作者，↩ 选一位即按该作者筛选，再选一位加上。
#[test]
fn the_author_menu_lists_every_author_and_can_be_searched() {
    use crate::app::PopupRow;
    use winit::keyboard::{Key, NamedKey};
    let Some((mut app, root)) = git_app("git-log-author-search", false) else {
        return;
    };
    history_repo(&root, 10);
    app.git_refresh();
    show_all_log(&mut app);
    // 只看最近一天：取回来的提交没有几位作者，菜单里仍然列出仓库里的全部作者。
    app.press_log(LogTarget::Date);
    app.run_menu_command(MenuCommand::LogSince(1));
    paint(&mut app);
    app.press_log(LogTarget::Author);
    let authors = app.git.log.authors.clone();
    assert!(authors.len() >= 3, "{authors:?}");
    assert!(authors.iter().all(|a| !a.contains('<')), "{authors:?}");
    let titles = |app: &App| -> Vec<String> {
        let rows = &app.popup().unwrap().levels[0].rows;
        rows.iter()
            .filter_map(|r| match r {
                PopupRow::Item { label, .. } => Some(label.title.clone()),
                PopupRow::Query(q) => Some(format!("[{q}]")),
                _ => None,
            })
            .collect()
    };

    // 键入「ali」：只剩名字里有它的作者，以及第一行写出的键入文字。
    for c in "ali".chars() {
        assert!(app.popup_logical_key(&Key::Character(c.to_string().into())));
    }
    assert_eq!(titles(&app), ["[ali]", "Alice Chen"]);
    assert!(app.popup_logical_key(&Key::Named(NamedKey::Enter)));
    assert!(app.overlays.context_menu.is_none());
    let query = |app: &App| {
        app.editor
            .active()
            .unwrap()
            .log
            .as_deref()
            .unwrap()
            .query
            .clone()
    };
    assert_eq!(query(&app).authors, ["Alice Chen"]);
    let view = app.editor.active().unwrap().log.as_deref().unwrap();
    assert!(!view.commits.is_empty());
    assert!(view.commits.iter().all(|c| c.author == "Alice Chen"));

    // 再打开、键入「张」，选上以后两位作者都列出；「全部作者」清空。
    paint(&mut app);
    app.press_log(LogTarget::Author);
    assert!(app.popup_logical_key(&Key::Character("张".into())));
    assert_eq!(titles(&app), ["[张]", "张伟"]);
    assert!(app.popup_logical_key(&Key::Named(NamedKey::Enter)));
    assert_eq!(query(&app).authors, ["Alice Chen", "张伟"]);
    let view = app.editor.active().unwrap().log.as_deref().unwrap();
    assert!(
        view.commits
            .iter()
            .all(|c| ["Alice Chen", "张伟"].contains(&c.author.as_str()))
    );
    paint(&mut app);
    app.press_log(LogTarget::Author);
    app.run_menu_command(MenuCommand::LogAuthor(0));
    assert!(query(&app).authors.is_empty());
}

/// 日志页中 `target` 的点击范围：最后一次 `paint` 记下的（`hit` 返回首个命中的）。
fn hit(app: &App, target: &LogTarget) -> Option<Rect> {
    app.hits
        .log
        .iter()
        .find(|(_, t)| t == target)
        .map(|(rect, _)| *rect)
}

/// 分支面板顶部的搜索框按名字筛选面板里的各行：只剩匹配的分支和它们所在的一组，面板回到顶部；
/// 清空以后恢复，包括「全部分支」。
#[test]
fn the_branch_panel_can_be_searched() {
    use crate::app::git::branch::picker::BranchRow;
    let Some((mut app, root)) = git_app("git-log-branch-search", false) else {
        return;
    };
    history_repo(&root, 10);
    app.git_refresh();
    show_all_log(&mut app);
    app.layout.editor.w = app.layout.editor.w.max(1200.0);
    paint(&mut app);
    let all = app.log_branch_rows().len();
    assert!(matches!(app.log_branch_rows()[0], BranchRow::All));

    // 点搜索框以后键入的文字进它，各行只剩名字匹配的。
    let (rect, _) = *app
        .hits
        .log
        .iter()
        .find(|(_, t)| *t == LogTarget::BranchFilter)
        .unwrap();
    app.pointer.mouse = (rect.x + 30.0, rect.y + rect.h / 2.0);
    app.press_log(LogTarget::BranchFilter);
    assert_eq!(app.focus, Focus::LogBranchFilter);
    app.edit_field(|f| f.insert("LOGIN"));
    let rows = app.log_branch_rows();
    assert!(rows.len() < all);
    assert!(rows.iter().all(|row| !matches!(row, BranchRow::All)));
    let named: Vec<&str> = rows
        .iter()
        .filter_map(|row| match row {
            BranchRow::Branch { index, .. } => Some(app.git.branches[*index].name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(named, ["feature/login"]);
    assert!(app.git.log.branches_scroll == 0.0);

    // 搜索框下面是各行：滚到最上面时第一行也不会盖住它。
    let field = hit(&app, &LogTarget::BranchFilter).unwrap();
    let first = hit(&app, &LogTarget::BranchRow(0)).unwrap();
    assert!(field.y >= app.log_parts().branches.y);
    assert!(first.y >= field.bottom());

    // 一个也不匹配时面板不画行（只剩分支选择器的「新建分支」，面板不画它）。
    app.edit_field(|f| {
        f.set_text("zzz");
        true
    });
    let rows = app.log_branch_rows();
    assert!(rows.iter().all(|r| matches!(r, BranchRow::Create(_))));
    paint(&mut app);
    assert!(hit(&app, &LogTarget::BranchRow(0)).is_none());

    // 清空以后面板恢复原样（「全部分支」在最上面）。
    app.edit_field(|f| {
        f.set_text("");
        true
    });
    assert_eq!(app.log_branch_rows().len(), all);
    assert!(matches!(app.log_branch_rows()[0], BranchRow::All));
}

/// 画出具名仓库的日志页，供人工查看分支面板的搜索框和筛选后的各行。
#[test]
#[ignore = "截图，手动运行"]
fn shoot_log_branch_filter() {
    for dark in [false, true] {
        let theme = if dark { "dark" } else { "light" };
        let Some((mut app, root)) = git_app_in(&format!("log-branch-filter-{theme}"), false, dark)
        else {
            return;
        };
        history_repo(&root, 10);
        for name in ["feature/login-remember", "release/1.0"] {
            git_in(&root, &["branch", name]);
        }
        app.git_refresh();
        show_all_log(&mut app);
        app.layout.editor.w = app.layout.editor.w.max(1200.0);
        paint(&mut app);
        if let Some((rect, _)) = app
            .hits
            .log
            .iter()
            .find(|(_, t)| *t == LogTarget::BranchFilter)
            .copied()
        {
            app.pointer.mouse = (rect.x + 30.0, rect.y + rect.h / 2.0);
            app.press_log(LogTarget::BranchFilter);
            app.edit_field(|f| f.insert("login"));
            shoot(&mut app, &format!("log-branch-filter-{theme}"));
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
