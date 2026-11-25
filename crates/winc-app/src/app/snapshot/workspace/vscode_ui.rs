//! VS Code 工作台布局与交互的端到端测试：默认主题、默认快捷键、顶栏命令中心、活动栏
//! 折叠/展开与设置齿轮、布局控制按钮（终端/分屏）以及顶部面包屑导航栏。

use super::*;

#[test]
fn vscode_themes_and_keymap_are_defaults() {
    assert!(std::ptr::eq(
        crate::theme::DARK_THEMES[0].palette,
        &crate::theme::VSCODE_DARK_MODERN
    ));
    assert!(std::ptr::eq(
        crate::theme::LIGHT_THEMES[0].palette,
        &crate::theme::VSCODE_LIGHT_MODERN
    ));
    assert_eq!(
        crate::keymap::Preset::default(),
        crate::keymap::Preset::VsCode
    );
    // VS Code Dark+ 使用经典蓝色状态栏（#007ACC）与纯白状态栏文字（#FFFFFF）。
    let plus = &crate::theme::VSCODE_DARK_PLUS;
    assert_eq!(plus.status_bar_bg, winc_render::Color::hex(0x007ACC));
    assert_eq!(plus.status_bar_fg, winc_render::Color::hex(0xFFFFFF));
    assert_ne!(plus.status_bar_bg, plus.title_bar_bg);
}

#[test]
fn vscode_workbench_controls_and_activity_bar_work() {
    let dir = sample_folder("vscode-workbench");
    let mut app = app_with(&dir, true);
    app.open_path(&dir.join("short.rs"));
    paint(&mut app);

    let center = |r: winc_render::Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);

    // 1. 顶部面包屑导航栏位于编辑区标签栏正下方、编辑区正上方。
    let l = app.layout;
    assert_eq!(l.breadcrumbs.h, crate::theme::BREADCRUMBS_H);
    assert!((l.breadcrumbs.y - l.top_bar.bottom()).abs() <= 0.5);
    assert!((l.editor.y - l.breadcrumbs.bottom()).abs() <= 0.5);
    assert!(!app.hits.crumbs.is_empty());

    // 2. 点击顶栏居中命令中心打开快速打开面板（Quick Open）。
    assert!(l.command_center.w > 100.0);
    click(&mut app, center(l.command_center));
    assert!(app.overlays.picker.open);
    app.close_picker();
    paint(&mut app);

    // 3. 点击活动栏底部设置齿轮打开设置浮层。
    let settings_btn = app.layout.activity_settings;
    assert!(settings_btn.w > 0.0);
    click(&mut app, center(settings_btn));
    assert!(app.settings.open);
    app.close_settings();
    paint(&mut app);

    // 4. VS Code 活动栏习惯：再次点击已激活图标折叠侧栏，再点一次展开；折叠时点搜索图标直接展开到搜索。
    assert!(app.panels.sidebar.visible);
    assert_eq!(app.panels.sidebar.view, crate::app::SidebarView::Files);
    let files_tab = app
        .sidebar_tabs()
        .into_iter()
        .find(|(_, v)| *v == crate::app::SidebarView::Files)
        .unwrap()
        .0;
    let search_tab = app
        .sidebar_tabs()
        .into_iter()
        .find(|(_, v)| *v == crate::app::SidebarView::Search)
        .unwrap()
        .0;
    click(&mut app, center(files_tab));
    assert!(!app.panels.sidebar.visible);
    paint(&mut app);
    click(&mut app, center(search_tab));
    assert!(app.panels.sidebar.visible);
    assert_eq!(app.panels.sidebar.view, crate::app::SidebarView::Search);
    paint(&mut app);

    // 5. 顶栏右侧布局按钮：切换终端面板、向右拆分编辑区。
    let terminal_before = app.terminal.open;
    let panel_btn = app.layout.panel_button;
    click(&mut app, center(panel_btn));
    assert_ne!(app.terminal.open, terminal_before);
    paint(&mut app);

    assert!(!app.editor.panes.is_split());
    let split_btn = app.layout.split_button;
    click(&mut app, center(split_btn));
    assert!(app.editor.panes.is_split());

    let _ = std::fs::remove_dir_all(&dir);
}
