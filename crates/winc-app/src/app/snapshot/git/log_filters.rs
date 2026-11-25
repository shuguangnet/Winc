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

    // 列配置状态与作者宽度助手保持兼容可持久化。
    assert!(app.log_column_shown(LogColumn::Author) && app.log_column_shown(LogColumn::Hash));
    app.toggle_log_column(LogColumn::Hash);
    assert!(!app.log_column_shown(LogColumn::Hash));
    assert_eq!(
        app.state.log_columns.as_deref(),
        Some(&["author".into(), "date".into()][..])
    );
    app.toggle_log_column(LogColumn::Hash);
    assert!(app.log_column_shown(LogColumn::Hash));
    assert_eq!(
        app.state.log_columns.as_deref(),
        Some(&["author".into(), "date".into(), "hash".into()][..])
    );
    assert_eq!(app.log_author_show(1.0), 0.0);
    app.drag_log_author_edge(100.0, (100.0, AUTHOR_DEFAULT));
    assert_eq!(app.log_author_show(1.0), 1.0);
    app.drag_log_author_edge(60.0, (100.0, AUTHOR_DEFAULT));
    assert_eq!(app.git.log.author_w, Some(AUTHOR_DEFAULT + 40.0));
    app.press_log_author_edge(100.0, true);
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

/// 画出自定义筛选后的日志页：作者、时间由对话框输入，日期下拉里写输入的原文，分支选择器里
/// 输入修订时最后一行是「按…筛选」。
#[test]
#[ignore = "截图，手动运行"]
fn shoot_log_typed_filters() {
    for dark in [false, true] {
        let theme = if dark { "dark" } else { "light" };
        let Some((mut app, root)) = git_app_in(&format!("log-typed-{theme}"), false, dark) else {
            return;
        };
        history_repo(&root, 10);
        app.git_refresh();
        show_all_log(&mut app);
        app.layout.editor.w = app.layout.editor.w.max(1200.0);
        paint(&mut app);
        app.open_log_author_dialog();
        app.overlays
            .form_dialog
            .as_mut()
            .unwrap()
            .name
            .set_text("alice");
        app.submit_form_dialog();
        app.open_log_since_dialog();
        let dialog = app.overlays.form_dialog.as_mut().unwrap();
        dialog.name.set_text("3 weeks ago");
        shoot(&mut app, &format!("log-typed-dialog-{theme}"));
        app.submit_form_dialog();
        shoot(&mut app, &format!("log-typed-since-{theme}"));
        app.press_log(LogTarget::Branch);
        app.overlays.picker.input.set_text("HEAD~4");
        app.branch_picker_refresh();
        shoot(&mut app, &format!("log-typed-branch-{theme}"));
        app.close_picker();
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// 详情标题行的放大镜打开「查找文件」：键入的文字按路径筛选选中提交改动的文件，标题行写成
/// 「留下来的 / 全部」；关掉搜索框以后文件列表恢复原样。
#[test]
fn the_log_details_can_be_filtered_by_file_path() {
    let Some((mut app, root)) = git_app("git-log-find-file", false) else {
        return;
    };
    history_repo(&root, 10);
    multi_file_commit(&root);
    app.git_refresh();
    show_all_log(&mut app);
    app.layout.editor.w = app.layout.editor.w.max(1200.0);
    paint(&mut app);
    // 最新的提交排在最上面，选中它看详情里的四个文件。
    app.press_log(LogTarget::Row(0));
    paint(&mut app);
    let all = app.log_file_order();
    assert_eq!(all.len(), 4, "{all:?}");

    // 1:1 `gitHistoryVSCode`：详情右上角始终直接显示 `Find file` 输入框（位于平铺/分组按钮左侧）。
    let field = hit(&app, &LogTarget::FileFilter).unwrap();
    let layout = hit(&app, &LogTarget::FilesLayout).unwrap();
    assert!(field.right() <= layout.x);
    let details = app.log_parts().details;
    assert!(field.y >= details.y && field.bottom() <= details.y + 44.0);
    let middle = (field.x + 20.0, field.y + field.h / 2.0);
    click(&mut app, middle);
    assert_eq!(app.focus, Focus::LogFileFilter);
    paint(&mut app);

    // 键入目录名：只剩这个目录下的两个文件，按目录分组时另一组的标题也不出现。
    app.edit_field(|f| f.insert("DOCS/"));
    paint(&mut app);
    let filtered = app.log_file_order();
    assert_eq!(filtered.len(), 2, "{filtered:?}");
    let named: Vec<String> = filtered
        .iter()
        .map(|i| {
            let view = app.editor.active().unwrap().log.as_deref().unwrap();
            view.details.as_ref().unwrap().files.as_ref().unwrap()[*i]
                .path
                .display()
                .to_string()
        })
        .collect();
    assert!(named.iter().all(|p| p.contains("docs/")), "{named:?}");
    // 被筛掉的文件不再记录点击范围。
    for i in 0..all.len() {
        let shown = filtered.contains(&i);
        assert_eq!(hit(&app, &LogTarget::File(i)).is_some(), shown, "文件 {i}");
    }

    // 一个也不匹配时文件列表空着，写一行「没有匹配的文件」；点这一行清掉键入的文字。
    app.edit_field(|f| {
        f.set_text("zzz");
        true
    });
    paint(&mut app);
    assert!(app.log_file_order().is_empty());
    for i in 0..all.len() {
        assert!(hit(&app, &LogTarget::File(i)).is_none(), "文件 {i}");
    }
    let empty = hit(&app, &LogTarget::ClearFileFilter).unwrap();
    assert!(empty.y >= field.bottom());
    click(&mut app, (empty.x + 20.0, empty.y + empty.h / 2.0));
    paint(&mut app);
    assert_eq!(app.log_file_order().len(), 4);
    assert!(hit(&app, &LogTarget::ClearFileFilter).is_none());
    assert!(app.git.log.file_filter.text().is_empty());

    // 新开一个日志标签（当前文件的历史）时文字清空：上一个标签的筛选不带过去。
    click(&mut app, middle);
    app.edit_field(|f| f.insert("docs/"));
    app.git_file_history(&root.join("src/alpha.rs"));
    assert!(app.git.log.file_filter.text().is_empty());
}

/// 在 [`history_repo`] 之上再加一次改动 `src`、`docs` 下各两个文件的提交：详情里的「查找文件」
/// 按目录筛时，两个组的文件和标题都看得出区别。
fn multi_file_commit(root: &Path) {
    std::fs::create_dir_all(root.join("docs")).unwrap();
    for (path, text) in [
        ("src/alpha.rs", "pub fn alpha() {}\n"),
        ("src/beta.rs", "pub fn beta() {}\n"),
        ("docs/guide.md", "# 使用说明\n"),
        ("docs/notes.md", "# 备注\n"),
    ] {
        std::fs::write(root.join(path), text).unwrap();
    }
    git_in(root, &["add", "."]);
    git_in(
        root,
        &[
            "-c",
            "user.name=Alice Chen",
            "-c",
            "user.email=alice@example.com",
            "commit",
            "-q",
            "-m",
            "文档：补充说明",
        ],
    );
}

/// 画出具名仓库的日志页，供人工查看详情底部的「查找文件」和筛选后的文件列表。
#[test]
#[ignore = "截图，手动运行"]
fn shoot_log_find_file() {
    for dark in [false, true] {
        let theme = if dark { "dark" } else { "light" };
        let Some((mut app, root)) = git_app_in(&format!("log-find-file-{theme}"), false, dark)
        else {
            return;
        };
        history_repo(&root, 10);
        multi_file_commit(&root);
        app.git_refresh();
        show_all_log(&mut app);
        app.layout.editor.w = app.layout.editor.w.max(1200.0);
        paint(&mut app);
        app.press_log(LogTarget::Row(0));
        paint(&mut app);
        if let Some((rect, _)) = app
            .hits
            .log
            .iter()
            .find(|(_, t)| *t == LogTarget::FileFilter)
            .copied()
        {
            let middle = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            click(&mut app, middle);
            app.edit_field(|f| f.insert("docs/"));
            shoot(&mut app, &format!("log-find-file-{theme}"));
            // 一个也不匹配时的提示行。
            app.edit_field(|f| {
                f.set_text("zzz");
                true
            });
            shoot(&mut app, &format!("log-find-file-empty-{theme}"));
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// 切换到另一个目录时「查找文件」也收起来：旧目录的筛选不该藏起新目录的文件。
#[test]
fn switching_folders_clears_the_file_filter() {
    let Some((mut app, root)) = git_app("git-log-find-file-switch", false) else {
        return;
    };
    history_repo(&root, 10);
    app.git_refresh();
    show_all_log(&mut app);
    app.layout.editor.w = app.layout.editor.w.max(1200.0);
    paint(&mut app);
    app.press_log(LogTarget::Row(0));
    paint(&mut app);
    let field = hit(&app, &LogTarget::FileFilter).unwrap();
    click(&mut app, (field.x + 20.0, field.y + field.h / 2.0));
    app.edit_field(|f| f.insert("src/"));
    assert_eq!(app.git.log.file_filter.text(), "src/");

    let other = search_folder("git-log-find-file-target");
    app.open_path(&other);
    assert_ne!(app.git.root.as_deref(), Some(root.as_path()));
    assert!(!app.git.log.find_files);
    assert!(app.git.log.file_filter.text().is_empty());
    assert_ne!(app.focus, Focus::LogFileFilter);

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&other);
}

/// 输入筛选条件：作者、时间在对话框里填，分支筛选在选择器里输入修订（提交号、`HEAD~3` 等）。
#[test]
fn log_filters_can_be_typed_in() {
    let Some((mut app, root)) = git_app("git-log-typed-filters", false) else {
        return;
    };
    history_repo(&root, 10);
    app.git_refresh();
    show_all_log(&mut app);
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
    let all = listed(&app);
    assert_eq!(all, count(&root, &[]));

    // 作者：菜单最后是「其他作者…」，填名字或邮箱的一部分就能筛（不分大小写）。
    app.open_context_menu(MenuTarget::LogAuthor, (0.0, 0.0));
    app.run_menu_command(MenuCommand::LogAuthorOther);
    let dialog = app.overlays.form_dialog.as_mut().unwrap();
    dialog.name.set_text("alice");
    app.submit_form_dialog();
    assert_eq!(query(&app).authors, ["alice"]);
    let alice = count(
        &root,
        &["--author=alice", "--regexp-ignore-case", "--fixed-strings"],
    );
    assert!(alice > 0 && alice < all);
    assert_eq!(listed(&app), alice);
    // 再填一次同一个作者不会重复。
    app.open_log_author_dialog();
    app.overlays
        .form_dialog
        .as_mut()
        .unwrap()
        .name
        .set_text("alice");
    app.submit_form_dialog();
    assert_eq!(query(&app).authors, ["alice"]);
    app.open_context_menu(MenuTarget::LogAuthor, (0.0, 0.0));
    app.run_menu_command(MenuCommand::LogAuthor(0));
    assert_eq!(listed(&app), all);

    // 时间：菜单最后是「其他时间…」，填 `git log --since` 认识的时间；和「最近几天」只用一个。
    let before = count(&root, &["--since=7.days.ago"]);
    app.open_context_menu(MenuTarget::LogDate, (0.0, 0.0));
    app.run_menu_command(MenuCommand::LogSinceOther);
    let dialog = app.overlays.form_dialog.as_mut().unwrap();
    dialog.name.set_text(" 7.days.ago ");
    app.submit_form_dialog();
    let after = count(&root, &["--since=7.days.ago"]);
    assert_eq!(query(&app).since.as_deref(), Some("7.days.ago"));
    assert_eq!(query(&app).since_days, None);
    assert!([before, after].contains(&listed(&app)));
    // 选「最近 30 天」时自定义的时间不再留着。
    app.open_context_menu(MenuTarget::LogDate, (0.0, 0.0));
    app.run_menu_command(MenuCommand::LogSince(30));
    assert_eq!(query(&app).since, None);
    assert_eq!(query(&app).since_days, Some(30));
    app.run_menu_command(MenuCommand::LogSince(0));
    assert_eq!(listed(&app), all);

    // 分支：选择器里输入的不是已有的分支或标签时，最后一行是「按…筛选」，回车按这个修订筛。
    app.press_log(LogTarget::Branch);
    app.overlays.picker.input.set_text("HEAD~3");
    app.branch_picker_refresh();
    let rows = app.overlays.picker.branch_rows.clone().unwrap();
    assert_eq!(rows.last(), Some(&BranchRow::Revision("HEAD~3".into())));
    app.overlays.picker.selected = rows.len() - 1;
    app.branch_picker_accept();
    assert!(app.overlays.picker.branch_rows.is_none());
    assert_eq!(query(&app).branches, ["HEAD~3"]);
    let rev = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["log", "--oneline", "HEAD~3"])
        .output()
        .unwrap();
    let rev = String::from_utf8_lossy(&rev.stdout).lines().count();
    assert!(rev > 0 && rev < all);
    assert_eq!(listed(&app), rev);

    // 输入的是已有的分支名时没有这一行，仍然只是从列表里挑分支。
    app.press_log(LogTarget::Branch);
    app.overlays.picker.input.set_text("main");
    app.branch_picker_refresh();
    let rows = app.overlays.picker.branch_rows.clone().unwrap();
    assert!(!rows.iter().any(|r| matches!(r, BranchRow::Revision(_))));
    app.close_picker();

    let _ = std::fs::remove_dir_all(&root);
}
