//! 提交视图中贮藏列表的测试。

use super::*;

use crate::app::Drag;
use crate::app::git::commit::STASH_ROWS;

/// 截图：改动列表下面展开的储藏一组（`commit-stashes`），鼠标停在第一条上（「应用」和「⋯」），
/// 储藏六条、一组露出五行，右端有滚动条。
#[test]
#[ignore = "截图，手动运行"]
fn shoot_commit_stashes() {
    let Some((mut app, root)) = git_app("commit-stashes", false) else {
        return;
    };
    git_in(&root, &["add", "-A"]);
    git_in(&root, &["commit", "-q", "-m", "base"]);
    for (i, note) in [
        "改登录",
        "试一下新写法",
        "调日志刷新",
        "备份配置",
        "临时打点",
        "旧方案",
    ]
    .iter()
    .enumerate()
    {
        std::fs::write(root.join("src/main.rs"), format!("fn main() {{ {i}; }}\n")).unwrap();
        git_in(&root, &["stash", "push", "-q", "-m", note]);
    }
    std::fs::write(root.join("src/main.rs"), "fn main() { now(); }\n").unwrap();
    app.git_refresh();
    app.run_action(Action::ShowCommit);
    app.press_commit(CommitTarget::Stashes);
    paint(&mut app);
    let (row, _) = *(app.hits.commit.iter())
        .find(|(_, t)| *t == CommitTarget::Stash(0))
        .unwrap();
    app.on_mouse_move(row.x + 20.0, row.y + row.h / 2.0);
    shoot(&mut app, "commit-stashes");
    // 往下滚一行：右端出现细条，最上面那条滚出去，露出原本在下面的第六条。
    app.git.commit.stash_scroll = COMMIT_ROW;
    shoot(&mut app, "commit-stashes-scrolled");
    // 打开第一条储藏：它的文件列在下面，正在查看的文件处于选中状态。
    std::fs::write(root.join("notes.txt"), "draft\n").unwrap();
    git_in(&root, &["stash", "push", "-q", "-u", "-m", "带一个新文件"]);
    app.git_refresh();
    paint(&mut app);
    app.press_commit(CommitTarget::Stash(0));
    shoot(&mut app, "commit-stash-files");
    // 右键点列出的第一个文件：菜单里除了「显示差异」，还有只取这一个文件的「只取这个文件」。
    let place = {
        let (row, _) = *(app.hits.commit.iter())
            .find(|(_, t)| matches!(t, CommitTarget::StashFile(_)))
            .unwrap();
        (row.x + 20.0, row.y + row.h / 2.0)
    };
    right_click(&mut app, place);
    shoot(&mut app, "commit-stash-file-menu");
    let _ = std::fs::remove_dir_all(&root);
}

/// 改动列表下面的储藏一组：默认收起；展开后每条一行。点一下在储藏的预览标签里打开它的第一个文件，
/// 差异里的下一个文件在这条储藏的文件之间切换，一起收进去的未跟踪文件排在后面；悬停时的「应用」
/// 把改动放回工作区。
#[test]
fn stashes_are_listed_below_the_changes_and_open_their_files() {
    let Some((mut app, root)) = git_app("git-commit-stashes", false) else {
        return;
    };
    git_in(&root, &["add", "-A"]);
    git_in(&root, &["commit", "-q", "-m", "base"]);
    std::fs::write(root.join("src/main.rs"), "fn main() { stashed(); }\n").unwrap();
    std::fs::write(root.join("notes.txt"), "draft\n").unwrap();
    git_in(&root, &["stash", "push", "-q", "-u", "-m", "试一下新写法"]);
    app.git_refresh();
    app.run_action(Action::ShowCommit);
    paint(&mut app);
    let stash = app.git.stashes[0].clone();
    assert_eq!(stash.branch(), Some("main"));
    assert_eq!(stash.parents.len(), 3);

    // 收着时只有标题一行，在列表下面。
    let parts = app.commit_parts();
    let section = parts.stashes.unwrap();
    assert_eq!(
        section.h,
        COMMIT_ROW + 2.0 * crate::app::git::commit::STASH_GAP
    );
    assert!(parts.list.bottom() <= section.y);
    assert!(!(app.hits.commit.iter()).any(|(_, t)| *t == CommitTarget::Stash(0)));
    app.press_commit(CommitTarget::Stashes);
    paint(&mut app);
    assert!((app.hits.commit.iter()).any(|(_, t)| *t == CommitTarget::Stash(0)));

    // 点一下：两个文件，已跟踪的在前；预览标签里是第一个，标题写着储藏的名字。
    app.press_commit(CommitTarget::Stash(0));
    let view = app.git.commit.stash_view.clone().unwrap();
    let names: Vec<String> = (view.files.iter())
        .map(|(f, ..)| winc_editor::display_name(&f.path).into_owned())
        .collect();
    assert_eq!(names, ["main.rs", "notes.txt"]);
    assert!(view.files[1].1.is_none());
    let doc = app.editor.active().unwrap();
    assert!(doc.diff.is_some());
    assert_eq!(app.git.commit.stash_preview, Some(doc.id));
    assert!(doc.name().contains("stash@{0}"));
    assert!(app.diff_steps_files());
    // 下一个文件是一起收进去的未跟踪文件。
    app.step_diff_file(true);
    assert_eq!(app.git.commit.stash_view.as_ref().unwrap().at, 1);
    assert!(app.editor.active().unwrap().name().contains("notes.txt"));
    assert_eq!(app.next_stash_file(true), None);

    // 储藏那一行下面列出它的两个文件，点击第一个时预览标签切回该文件。
    use crate::app::StashLine;
    let lines = [StashLine::Stash(0), StashLine::File(0), StashLine::File(1)];
    assert_eq!(app.stash_lines(), lines);
    paint(&mut app);
    let section = app.commit_parts().stashes.unwrap();
    assert_eq!(
        section.h,
        4.0 * COMMIT_ROW + 2.0 * crate::app::git::commit::STASH_GAP
    );
    assert!((app.hits.commit.iter()).any(|(_, t)| *t == CommitTarget::StashFile(1)));
    app.press_commit(CommitTarget::StashFile(0));
    assert_eq!(app.git.commit.stash_view.as_ref().unwrap().at, 0);
    assert!(app.editor.active().unwrap().name().contains("main.rs"));

    // 再次点击这条储藏：文件收起，预览标签保留；再次点击时重新展开。
    app.press_commit(CommitTarget::Stash(0));
    assert_eq!(app.stash_lines(), [StashLine::Stash(0)]);
    assert!(app.editor.active().unwrap().name().contains("main.rs"));
    app.press_commit(CommitTarget::Stash(0));
    assert_eq!(app.stash_lines(), lines);
    // 关闭预览标签：文件不再列出。
    app.run_action(Action::CloseTab);
    paint(&mut app);
    assert_eq!(app.stash_lines(), [StashLine::Stash(0)]);

    // 「应用」：改动回到工作区，储藏还在。
    app.press_commit(CommitTarget::StashApply(0));
    app.git_refresh();
    assert!(root.join("notes.txt").exists());
    assert_eq!(app.git.stashes.len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

/// 储藏一组像抽屉一样拉开：途中一组的高度和箭头的角度都在两端之间，上面的改动列表跟着变矮，
/// 还在移动的行不响应点击；停稳以后各行才能点。
#[test]
fn the_stash_group_opens_like_a_drawer() {
    let Some((mut app, root)) = git_app("git-commit-stash-drawer", false) else {
        return;
    };
    git_in(&root, &["add", "-A"]);
    git_in(&root, &["commit", "-q", "-m", "base"]);
    std::fs::write(root.join("src/main.rs"), "fn main() { stashed(); }\n").unwrap();
    git_in(&root, &["stash", "push", "-q", "-m", "试一下新写法"]);
    std::fs::write(root.join("src/main.rs"), "fn main() { now(); }\n").unwrap();
    app.git_refresh();
    app.run_action(Action::ShowCommit);
    paint(&mut app);
    let closed = app.commit_parts().stashes.unwrap();
    app.press_commit(CommitTarget::Stashes);
    let open = app.commit_parts().stashes.unwrap();
    assert_eq!(open.h, closed.h + COMMIT_ROW);

    app.git.commit.stashes.motion =
        Some((closed.h, Clock::frozen(TREE_ANIMATION / 2, TREE_ANIMATION)));
    let parts = app.commit_parts();
    let half = parts.stashes.unwrap();
    assert!(closed.h < half.h && half.h < open.h, "{half:?}");
    assert_eq!(half.bottom(), open.bottom());
    // 提交图在改动列表和储藏之间，列表到它为止。
    assert_eq!(
        parts.list.bottom(),
        parts.graph.map_or(half.y, |graph| graph.y)
    );
    assert!(0.0 < app.stash_chevron() && app.stash_chevron() < 1.0);
    assert!(app.animating());
    paint(&mut app);
    let has = |app: &App, target| (app.hits.commit.iter()).any(|(_, t)| *t == target);
    assert!(has(&app, CommitTarget::Stashes));
    assert!(!has(&app, CommitTarget::Stash(0)));

    app.git.commit.stashes.motion = Some((closed.h, Clock::frozen(TREE_ANIMATION, TREE_ANIMATION)));
    app.settle_animations();
    assert!(app.git.commit.stashes.motion.is_none());
    paint(&mut app);
    assert!(has(&app, CommitTarget::Stash(0)));
    assert_eq!(app.stash_chevron(), 1.0);
    let _ = std::fs::remove_dir_all(&root);
}

/// 储藏多于一组露得下的行数时在组内滚动，而不是最后一行换成「还有 N 条」的菜单：滚轮滚的是储藏
/// 一组（不是上面的改动列表），滚出范围的行不再响应点击。
#[test]
fn many_stashes_scroll_inside_the_group() {
    let Some((mut app, root)) = git_app("git-commit-stash-scroll", false) else {
        return;
    };
    git_in(&root, &["add", "-A"]);
    git_in(&root, &["commit", "-q", "-m", "base"]);
    for i in 0..8 {
        std::fs::write(root.join("src/main.rs"), format!("fn main() {{ {i}; }}\n")).unwrap();
        git_in(&root, &["stash", "push", "-q", "-m", &format!("第 {i} 条")]);
    }
    std::fs::write(root.join("src/main.rs"), "fn main() { now(); }\n").unwrap();
    app.git_refresh();
    app.run_action(Action::ShowCommit);
    app.press_commit(CommitTarget::Stashes);
    paint(&mut app);

    // 八条都列在下面，一组只露出五行，其余滚动才看得到。
    assert_eq!(app.git.stashes.len(), 8);
    assert_eq!(app.stash_lines().len(), 8);
    assert_eq!(app.stash_visible_rows(), STASH_ROWS);
    assert_eq!(app.stash_scroll_max(), 3.0 * COMMIT_ROW);
    let rows = app.stash_rows_area().expect("展开的储藏一组");
    assert_eq!(rows.h, STASH_ROWS as f32 * COMMIT_ROW);
    let has = |app: &App, target| (app.hits.commit.iter()).any(|(_, t)| *t == target);
    assert!(has(&app, CommitTarget::Stash(4)));
    assert!(!has(&app, CommitTarget::Stash(5)), "露不下的行不响应点击");

    // 滚轮落在储藏一组上时滚的是它，上面的改动列表不动。
    let (x, y) = (rows.x + 30.0, rows.y + 20.0);
    app.pointer.mouse = (x, y);
    assert_eq!(app.scroll_target_at(x, y), ScrollTarget::Stash);
    // 右边细条：悬停、按住它同样能滚（拖动以后细条才一直显示）。
    let (bar_x, bar_y) = (rows.right() - 3.0, rows.y + 20.0);
    app.on_mouse_move(bar_x, bar_y);
    assert_eq!(app.pointer.hover, Hover::Scrollbar(Scrollbar::Stash));
    app.on_mouse_down(MouseButton::Left);
    assert!(
        matches!(
            app.pointer.drag,
            Drag::Scrollbar {
                bar: Scrollbar::Stash,
                ..
            }
        ),
        "按在储藏一组的细条上拖动的是它"
    );
    app.on_mouse_move(bar_x, bar_y + 6.0);
    app.on_mouse_up(MouseButton::Left);
    assert!(
        app.git.commit.stash_scroll > 0.0,
        "拖动细条把储藏一组往下滚"
    );
    let changes = app.git.commit.scroll;
    app.on_wheel(MouseScrollDelta::LineDelta(0.0, -1.0));
    paint(&mut app);
    assert!(app.git.commit.stash_scroll > 0.0);
    for _ in 0..40 {
        app.on_wheel(MouseScrollDelta::LineDelta(0.0, -5.0));
    }
    paint(&mut app);
    assert_eq!(app.git.commit.stash_scroll, app.stash_scroll_max());
    assert_eq!(app.git.commit.scroll, changes, "改动列表的滚动位置不变");
    assert!(has(&app, CommitTarget::Stash(7)), "滚到底能看到最后一条");
    assert!(!has(&app, CommitTarget::Stash(0)));

    // 往回滚过头停在最前面，第一条又露出来。
    for _ in 0..40 {
        app.on_wheel(MouseScrollDelta::LineDelta(0.0, 5.0));
    }
    paint(&mut app);
    assert_eq!(app.git.commit.stash_scroll, 0.0);
    assert!(has(&app, CommitTarget::Stash(0)));
    let _ = std::fs::remove_dir_all(&root);
}

/// 按住储藏的一行上下拖就滚动，不用拖右边的细条；松开时没移动过才算点击，仍然打开这条储藏。
#[test]
fn a_stash_row_can_be_dragged_to_scroll_and_still_clicks() {
    let Some((mut app, root)) = git_app("git-commit-stash-pan", false) else {
        return;
    };
    git_in(&root, &["add", "-A"]);
    git_in(&root, &["commit", "-q", "-m", "base"]);
    for i in 0..8 {
        std::fs::write(root.join("src/main.rs"), format!("fn main() {{ {i}; }}\n")).unwrap();
        git_in(&root, &["stash", "push", "-q", "-m", &format!("第 {i} 条")]);
    }
    std::fs::write(root.join("src/main.rs"), "fn main() { now(); }\n").unwrap();
    app.git_refresh();
    app.run_action(Action::ShowCommit);
    app.press_commit(CommitTarget::Stashes);
    paint(&mut app);
    let (row, _) = *(app.hits.commit.iter())
        .find(|(_, t)| *t == CommitTarget::Stash(1))
        .unwrap();
    let (x, y) = (row.x + 40.0, row.y + row.h / 2.0);
    app.on_mouse_move(x, y);
    app.on_mouse_down(MouseButton::Left);
    assert!(
        matches!(app.pointer.drag, Drag::StashPan { .. }),
        "按住一行开始拖动，不马上打开储藏"
    );
    assert!(app.git.commit.stash_view.is_none());
    // 往上拖两行：内容跟着走两行，松开停在新的位置。
    app.on_mouse_move(x, y - 2.0 * COMMIT_ROW);
    assert_eq!(app.git.commit.stash_scroll, 2.0 * COMMIT_ROW);
    app.on_mouse_up(MouseButton::Left);
    assert_eq!(app.git.commit.stash_scroll, 2.0 * COMMIT_ROW);
    assert!(
        app.git.commit.stash_view.is_none(),
        "拖动过就不算点击，不打开储藏"
    );

    // 拖动不改变选中的那条储藏：没移动过就按下、松开，仍然是点击。
    paint(&mut app);
    let (row, _) = *(app.hits.commit.iter())
        .find(|(_, t)| *t == CommitTarget::Stash(2))
        .unwrap();
    let (x, y) = (row.x + 40.0, row.y + row.h / 2.0);
    app.on_mouse_move(x, y);
    app.on_mouse_down(MouseButton::Left);
    app.on_mouse_up(MouseButton::Left);
    let view = app.git.commit.stash_view.clone().expect("点开储藏");
    assert_eq!(view.at, 0);
    assert!(
        app.editor
            .active()
            .is_some_and(|doc| doc.name().contains("stash@{"))
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 储藏下面列出的一个文件可以只取它：右键菜单里「只取这个文件」把它从储藏放回工作区和暂存区，
/// 其余文件不动，储藏还在；储藏里删掉了它的那一项是「只删这个文件」，取出就是把工作区里的它
/// 删掉。
#[test]
fn a_file_can_be_taken_out_of_a_stash_on_its_own() {
    let Some((mut app, root)) = git_app("git-commit-stash-take-file", false) else {
        return;
    };
    std::fs::write(root.join("notes.txt"), "draft\n").unwrap();
    git_in(&root, &["add", "-A"]);
    git_in(&root, &["commit", "-q", "-m", "base"]);
    // 一个改内容、一个删掉，一起收进储藏：工作区里两个文件都回到提交时的样子。
    std::fs::write(root.join("src/main.rs"), "fn main() { stashed(); }\n").unwrap();
    std::fs::remove_file(root.join("notes.txt")).unwrap();
    git_in(&root, &["stash", "push", "-q", "-m", "试一下"]);
    assert!(root.join("notes.txt").exists());
    std::fs::write(root.join("src/main.rs"), "fn main() { now(); }\n").unwrap();
    app.git_refresh();
    app.run_action(Action::ShowCommit);
    app.press_commit(CommitTarget::Stashes);
    paint(&mut app);
    app.press_commit(CommitTarget::Stash(0));
    paint(&mut app);

    let stash = app.git.stashes[0].clone();
    let at_of = |app: &App, name: &str| -> usize {
        (app.git.commit.stash_view.as_ref().unwrap().files.iter())
            .position(|(file, ..)| winc_editor::display_name(&file.path) == name)
            .unwrap()
    };
    let place_of = |app: &App, at: usize| -> (f32, f32) {
        let (row, _) = *(app.hits.commit.iter())
            .find(|(_, t)| *t == CommitTarget::StashFile(at))
            .unwrap();
        (row.x + 20.0, row.y + row.h / 2.0)
    };
    let (modified, removed) = (at_of(&app, "main.rs"), at_of(&app, "notes.txt"));
    assert_eq!(
        app.git.commit.stash_view.as_ref().unwrap().files[removed]
            .0
            .change,
        winc_editor::git::Change::Deleted
    );

    // 右键点改过内容的那个文件：菜单是这个文件的，菜单项按储藏里怎么变的写。
    let place = place_of(&app, modified);
    right_click(&mut app, place);
    let target = app
        .overlays
        .context_menu
        .as_ref()
        .map(|menu| menu.target.clone());
    assert!(
        matches!(
            target,
            Some(MenuTarget::StashFile {
                deleted: false,
                ref path,
                ..
            }) if *path == root.join("src/main.rs")
        ),
        "{target:?}"
    );
    let commands: Vec<_> = (menu_commands(&app).into_iter())
        .map(|(command, _)| command)
        .collect();
    assert_eq!(
        commands,
        [
            MenuCommand::GitShowDiff,
            MenuCommand::GitStashTakeFile(false),
            MenuCommand::CopyPath,
            MenuCommand::CopyRelativePath,
        ]
    );

    // 只取这一个文件：内容是储藏里的，储藏里被删掉的另一个文件没有跟着回来，储藏也还在。
    app.run_menu_command(MenuCommand::GitStashTakeFile(false));
    app.poll_branch_ops();
    assert_eq!(
        std::fs::read_to_string(root.join("src/main.rs")).unwrap(),
        "fn main() { stashed(); }\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("notes.txt")).unwrap(),
        "draft\n"
    );
    assert_eq!(app.git.stashes.len(), 1, "储藏还在");
    assert_eq!(app.git.stashes[0].oid, stash.oid);

    // 储藏里删掉了的那个文件：菜单写成「只删这个文件」，取出就是把它从工作区删掉。
    if app.git.commit.stash_files_folded {
        app.press_commit(CommitTarget::Stash(0));
    }
    paint(&mut app);
    let place = place_of(&app, at_of(&app, "notes.txt"));
    right_click(&mut app, place);
    let target = app
        .overlays
        .context_menu
        .as_ref()
        .map(|menu| menu.target.clone());
    assert!(
        matches!(target, Some(MenuTarget::StashFile { deleted: true, .. })),
        "{target:?}"
    );
    assert_eq!(
        MenuCommand::GitStashTakeFile(true).label(),
        winc_i18n::t!("menu-git-stash-take-deletion")
    );
    app.run_menu_command(MenuCommand::GitStashTakeFile(true));
    app.poll_branch_ops();
    assert!(!root.join("notes.txt").exists());
    assert_eq!(app.git.stashes.len(), 1, "储藏还在");
    let _ = std::fs::remove_dir_all(&root);
}
