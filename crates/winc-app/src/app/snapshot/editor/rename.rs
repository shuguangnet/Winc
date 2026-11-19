//! 符号重命名的测试：定义与用法一起改名，已打开的文档可撤销，未打开的文件写回磁盘。

use super::*;
use crate::keymap::Action;

#[test]
fn renaming_a_symbol_updates_open_and_closed_files() {
    let dir = search_folder("rename-symbol");
    // 两个文件都引用 `collect_ids`：src/main.rs 定义并调用，src/ids.rs 调用（先不打开）。
    std::fs::write(
        dir.join("src/main.rs"),
        "fn collect_ids() -> u32 {\n    1\n}\n\nfn main() {\n    let n = collect_ids();\n    println!(\"{n}\");\n}\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/ids.rs"),
        "use crate::collect_ids;\n\nfn call() -> u32 {\n    collect_ids()\n}\n",
    )
    .unwrap();
    let mut app = app_with(&dir, false);
    app.open_path(&dir.join("src/main.rs"));
    paint(&mut app);
    let count = |app: &App, path: &str, name: &str| {
        app.editor
            .documents
            .iter()
            .find(|d| d.path.as_deref().is_some_and(|p| p.ends_with(path)))
            .map(|d| {
                let text = d.text().rope().map_or_else(String::new, |r| r.to_string());
                text.matches(name).count()
            })
            .unwrap_or_else(|| {
                std::fs::read_to_string(dir.join(path))
                    .unwrap()
                    .matches(name)
                    .count()
            })
    };
    assert_eq!(count(&app, "src/main.rs", "collect_ids"), 2);

    // 光标放在定义上（第 0 行第 3 列）重命名：打开的文档与未打开的文件一起改名。
    app.select(3, 3);
    app.start_rename_symbol("collect_ids".into(), "gather_tokens".into());
    paint(&mut app);
    assert_eq!(
        count(&app, "src/main.rs", "collect_ids"),
        0,
        "打开的文档已改名"
    );
    assert_eq!(count(&app, "src/main.rs", "gather_tokens"), 2);
    assert_eq!(
        count(&app, "src/ids.rs", "gather_tokens"),
        2,
        "未打开的文件写回磁盘：use 一处、调用一处"
    );

    // 打开的文档：重命名是一次可撤销的修改。
    assert!(app.run_action(Action::Undo));
    paint(&mut app);
    assert_eq!(
        count(&app, "src/main.rs", "collect_ids"),
        2,
        "撤销后恢复旧名字"
    );

    // 新名字不是合法的标识符时拒绝执行，内容保持不变。
    app.select(3, 3);
    app.start_rename_symbol("collect_ids".into(), "1gather".into());
    paint(&mut app);
    assert_eq!(count(&app, "src/main.rs", "1gather"), 0);
    assert_eq!(count(&app, "src/main.rs", "collect_ids"), 2);

    let _ = std::fs::remove_dir_all(&dir);
}
