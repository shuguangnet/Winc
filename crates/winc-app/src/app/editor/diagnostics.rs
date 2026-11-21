//! 检查问题：在打开的目录里运行配置的检查命令（默认 `cargo check
//! --message-format=json`），解析出诊断（错误、警告）列在浮窗卡片中，选中一项跳到所在位置。
//!
//! 输出按行解析 JSON（rustc 风格的 `compiler-message`）：每个诊断取「主跨度」的文件、行列和
//! 消息文字；无法解析的行按普通输出跳过。命令的输出可达数 MB，解析在后台线程进行，完成后经
//! [`Loaded::Diagnostics`] 送回界面（与文件历史差异共用同一条结果通道）。

use std::path::{Path, PathBuf};
use std::process::Command;

use winc_config::Json;
use winc_i18n::t;

use crate::app::editor::definition::{Place, PlacesKind};
use crate::app::git::log::Loaded;

use super::*;

/// 一条诊断：文件、行列（均从 0 起）、级别和消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Diagnostic {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    pub level: String,
    pub message: String,
}

impl App {
    /// 运行检查命令（配置 `checkCommand`，默认 `cargo check --message-format=json`）并解析出
    /// 诊断，列在浮窗卡片中；没有打开目录时提示。正在运行时不再重复发起。
    pub(in crate::app) fn run_checks(&mut self) {
        if self.editor.workspace.is_none() {
            self.flash(t!("editor-open-folder-first"));
            return;
        }
        if self.diagnostics_running {
            self.flash(t!("editor-check-running"));
            return;
        }
        let root = self.editor.root().map(Path::to_path_buf);
        let Some(root) = root else {
            self.flash(t!("editor-open-folder-first"));
            return;
        };
        let command = self
            .config
            .check_command
            .clone()
            .unwrap_or_else(|| "cargo check --message-format=json".to_owned());
        self.diagnostics_running = true;
        self.flash(t!("editor-check-running"));
        let sender = self.git.log.results.0.clone();
        self.spawn_blocking_then(UserEvent::GitLog, move || {
            let result = run_check(&root, &command).map(|list| {
                let mut places: Vec<Place> = list
                    .into_iter()
                    .map(|diagnostic| Place {
                        path: diagnostic.path,
                        relative: String::new(),
                        line: diagnostic.line,
                        col: Some(diagnostic.col),
                        preview: format!("[{}] {}", diagnostic.level, diagnostic.message),
                        marks: Vec::new(),
                    })
                    .collect();
                // 错误在最前，同级按文件与行号。
                places.sort_by_key(|place| {
                    let level = place.preview.starts_with("[warning]");
                    (!level, place.relative.clone(), place.line)
                });
                places
            });
            let _ = sender.send(Loaded::Diagnostics { result });
        });
        self.request_redraw();
    }

    /// 收到检查的结果（见 [`Loaded::Diagnostics`]）：在浮窗卡片中列出全部诊断；没有问题时在
    /// 状态栏说明。
    pub(in crate::app) fn diagnostics_arrived(&mut self, places: Vec<Place>) {
        self.diagnostics_running = false;
        if places.is_empty() {
            self.flash(t!("editor-check-clean"));
            self.request_redraw();
            return;
        }
        self.show_places(PlacesKind::Diagnostics, String::new(), places);
        self.request_redraw();
    }
}

/// 运行检查命令并解析出诊断列表；命令无法启动或执行失败时返回原因。
fn run_check(root: &Path, command: &str) -> Result<Vec<Diagnostic>, String> {
    let mut parts = command.split_whitespace();
    let Some(program) = parts.next() else {
        return Err(t!("editor-check-invalid-command").to_owned());
    };
    let output = Command::new(program)
        .args(parts)
        .current_dir(root)
        .output()
        .map_err(|e| {
            t!(
                "editor-check-run-failed",
                command = command,
                error = e.to_string()
            )
            .to_owned()
        })?;
    Ok(parse_diagnostics(&String::from_utf8_lossy(&output.stdout)))
}

/// 从 `cargo check --message-format=json` 的输出里解析出各条诊断。
///
/// 每行是一个 JSON 对象：`reason` 为 `compiler-message` 时取消息里的「主跨度」（文件、行、
/// 列）和级别、消息文字；其余行（构建进度、普通输出）跳过。
fn parse_diagnostics(stdout: &str) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let Some(json) = winc_config::Json::parse_jsonc(line).map(|(json, _)| json) else {
            continue;
        };
        let Some(object) = json.as_object() else {
            continue;
        };
        if object.get("reason").and_then(Json::as_str) != Some("compiler-message") {
            continue;
        }
        let Some(message) = object.get("message").and_then(Json::as_object) else {
            continue;
        };
        let Some(spans) = message.get("spans").and_then(Json::as_array) else {
            continue;
        };
        let Some(primary) = spans
            .iter()
            .filter_map(|span| span.as_object())
            .find(|span| span.get("is_primary").and_then(Json::as_bool) == Some(true))
        else {
            continue;
        };
        let file = primary.get("file_name").and_then(Json::as_str);
        let line = primary.get("line_start").and_then(Json::as_number);
        let col = primary.get("column_start").and_then(Json::as_number);
        let (Some(file), Some(line), Some(col)) = (file, line, col) else {
            continue;
        };
        let level = message
            .get("level")
            .and_then(Json::as_str)
            .unwrap_or("note");
        let text = message.get("message").and_then(Json::as_str).unwrap_or("");
        out.push(Diagnostic {
            path: PathBuf::from(file),
            line: (line as usize).saturating_sub(1),
            col: (col as usize).saturating_sub(1),
            level: level.to_owned(),
            message: text.to_owned(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// rustc 风格的诊断输出：错误与警告各一条，夹着构建进度行。
    #[test]
    fn parses_rustc_json_messages() {
        let stdout = r#"{"reason":"compiler-artifact","target":{"name":"winc"}}
{"reason":"compiler-message","message":{"level":"error","message":"mismatched types","spans":[{"file_name":"src/main.rs","line_start":3,"column_start":5,"is_primary":true}],"rendered":"error[E0308]: mismatched types"},"children":[]}
{"reason":"compiler-message","message":{"level":"warning","message":"unused variable","spans":[{"file_name":"src/lib.rs","line_start":10,"column_start":9,"is_primary":true}]},"children":[]}
普通输出不是 JSON，跳过"#;
        let list = parse_diagnostics(stdout);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].path, PathBuf::from("src/main.rs"));
        assert_eq!(list[0].line, 2, "行号从 1 起换算成从 0 起");
        assert_eq!(list[0].col, 4);
        assert_eq!(list[0].level, "error");
        assert_eq!(list[0].message, "mismatched types");
        assert_eq!(list[1].level, "warning");
        assert_eq!(list[1].path, PathBuf::from("src/lib.rs"));
    }

    /// 没有主跨度（宏展开内部的辅助信息）的诊断整条跳过。
    #[test]
    fn diagnostics_without_a_primary_span_are_skipped() {
        let stdout = r#"{"reason":"compiler-message","message":{"level":"note","message":"help","spans":[]}}"#;
        assert!(parse_diagnostics(stdout).is_empty());
    }
}
