//! 符号重命名（`F2`）：把光标所在的名字（定义与全部用法）在项目里改成新名字。
//!
//! 流程：先弹对话框输入新名字，确认后走定义/用法搜索的同一套管线（[`DefinitionSearch`]，
//! 重命名模式下两轮剔除后按文件合并），最后把每个文件的命中替换成新名字——打开的文档就地
//! 编辑（一次可撤销的修改），未打开的文件原子写回磁盘。替换前逐处核对内容，搜索之后文件
//! 又被改过的匹配自动跳过。

use std::path::Path;

use winc_editor::usages::Filter;
use winc_editor::{Query, replace_in_document, replace_in_file};
use winc_i18n::t;

use crate::app::editor::definition::DefinitionSearch;
use crate::app::git::form::FormKind;

use super::*;

impl App {
    /// 重命名光标所在的符号（`F2`）：弹出对话框输入新名字。
    ///
    /// 没有打开目录时提示；光标不在一个名字上时给出淡色提示（[`name_at_caret`]）。
    pub(in crate::app) fn rename_symbol_prompt(&mut self) {
        if self.editor.workspace.is_none() {
            self.flash(t!("editor-open-folder-first"));
            return;
        }
        let Some(old) = self.name_at_caret() else {
            return;
        };
        let mut input = crate::field::TextField::default();
        input.set_text(&old);
        self.show_form_dialog(
            FormKind::RenameSymbol { name: old },
            input,
            crate::layout::DialogPart::Branch,
        );
    }

    /// 按下对话框的「重命名」：校验新名字后开始搜索旧名字的用法与定义，完成后替换。
    ///
    /// 新名字必须也是发起查找的文件的语言中的合法标识符，否则在状态栏说明。
    pub(in crate::app) fn start_rename_symbol(&mut self, old: String, new: String) {
        if new.is_empty() || new == old {
            return;
        }
        let Some(root) = self.editor.root().map(Path::to_path_buf) else {
            return;
        };
        let Some(doc) = self.editor.active() else {
            return;
        };
        let language = doc.highlighter.language();
        if Filter::new(&new, language).is_none() {
            self.flash(t!("editor-rename-invalid", name = new.as_str()));
            return;
        }
        let query = Query {
            text: old.clone(),
            case_sensitive: !Filter::ignores_case(&old, language),
            whole_word: true,
            ..Query::default()
        };
        let search = DefinitionSearch {
            engine: winc_editor::ProjectSearch::default(),
            name: old,
            filter: std::sync::Arc::new(
                Filter::new(&query.text, doc.highlighter.language())
                    .expect("旧名字已在打开对话框时校验为合法标识符"),
            ),
            filtering: None,
            language,
            from: doc.path.clone().unwrap_or_default(),
            at_caret: true,
            usages_of: None,
            rename_to: Some(new),
            receiver: None,
            origin: self.editor.active().map(|d| (d.id, d.selection.head)),
        };
        self.start_place_search(search, &root, &query);
    }

    /// 把搜索到的「用法 + 定义」替换成新名字：已打开的文档就地编辑，未打开的文件原子写回
    /// 磁盘，最后在状态栏汇总。
    pub(in crate::app) fn apply_rename(&mut self, search: DefinitionSearch, new_name: String) {
        let old = search.name.clone();
        let ignore_case = search.filter.ignore_case();
        let query = Query {
            text: old.clone(),
            case_sensitive: !ignore_case,
            whole_word: true,
            ..Query::default()
        };
        let (mut total, mut files, mut opened, mut failed) = (0usize, 0usize, 0usize, 0usize);
        for file in &search.engine.results {
            if file.matches.is_empty() {
                continue;
            }
            let applied = match self.document_at(&file.path) {
                Some(doc) => {
                    let n = replace_in_document(doc, &query, &new_name, &file.matches).unwrap_or(0);
                    opened += usize::from(n > 0);
                    n
                }
                None if file.lossy => {
                    failed += 1;
                    0
                }
                None => match replace_in_file(&file.path, &query, &new_name, &file.matches) {
                    Ok(n) => {
                        if n > 0 {
                            files += 1;
                        }
                        n
                    }
                    Err(_) => {
                        failed += 1;
                        0
                    }
                },
            };
            total += applied;
        }
        let mut message = t!(
            "editor-rename-done",
            count = total,
            files = files,
            old = old.as_str(),
            new = new_name.as_str()
        );
        if opened > 0 {
            message.push_str(&t!("workspace-replace-all-unsaved", opened = opened));
        }
        if failed > 0 {
            message.push_str(&t!("editor-rename-failed", count = failed));
        }
        self.flash(message);
        self.request_redraw();
    }
}

/// 合并「用法」与「定义」两轮剔除后的结果：同一个文件的匹配取并集（按行号与列范围去重），
/// 文件顺序与第一轮相同。
pub(in crate::app) fn merge_rename_matches(
    usage: Vec<winc_editor::FileResult>,
    defs: Vec<winc_editor::FileResult>,
) -> Vec<winc_editor::FileResult> {
    let mut merged = usage;
    let mut extra = defs;
    for file in &mut merged {
        let Some(other) = extra
            .iter_mut()
            .find(|e| e.path == file.path && e.relative == file.relative)
        else {
            continue;
        };
        file.matches.append(&mut other.matches);
        file.matches
            .sort_by_key(|m| (m.line, m.cols.start, m.cols.end));
        file.matches.dedup();
        file.lossy |= other.lossy;
    }
    // 只在定义一轮里出现的文件整体挪进来，保持它们的相对顺序。
    for other in extra {
        if !merged.iter().any(|file| file.path == other.path) {
            merged.push(other);
        }
    }
    merged
}
