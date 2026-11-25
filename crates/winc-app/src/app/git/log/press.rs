//! Git 日志标签的点击交互：提交行、快捷操作按钮、引用胶囊、文件列表与筛选栏的点击分发。

use winc_editor::git::Ref;
use winc_i18n::t;

use crate::app::input::context_menu::{MenuCommand, MenuTarget};
use crate::app::{App, Focus};

use super::{LogConfirmed, LogTarget, short};

impl App {
    /// 处理对日志标签中 `target` 的点击。
    pub(in crate::app) fn press_log(&mut self, target: LogTarget) {
        match target {
            LogTarget::Uncommitted => self.show_commit(),
            LogTarget::Row(row) | LogTarget::Avatar(row) => {
                self.focus = Focus::Editor;
                // `⌘` 点击另一个提交时，比较选中的提交与被点击的提交。
                let selected = self
                    .editor
                    .active()
                    .and_then(|d| d.log.as_deref()?.selected);
                match selected {
                    Some(at) if self.command_held() && at != row => {
                        self.compare_log_rows(at, row);
                    }
                    _ => {
                        self.clear_log_compare();
                        self.select_log_row(row);
                    }
                }
            }
            LogTarget::File(i) | LogTarget::FileComparePrevious(i) => self.open_log_file(i),
            LogTarget::FileDir(dir_idx) => self.toggle_log_file_dir(dir_idx),
            LogTarget::FilesFoldAll => self.toggle_log_files_fold_all(),
            LogTarget::FileView(i) => {
                if let Some(path) = self.log_file(i).map(|f| f.path.clone()) {
                    self.view_log_file_at_commit(path);
                }
            }
            LogTarget::FileCompareLocal(i) => {
                if let Some(path) = self.log_file(i).map(|f| f.path.clone()) {
                    self.compare_log_file_with_local(path);
                }
            }
            LogTarget::FileHistory(i) => {
                if let Some(path) = self.log_file(i).map(|f| f.path.clone()) {
                    self.git_file_history(&path);
                }
            }
            LogTarget::RowFilterAuthor(row) => {
                let author = self
                    .editor
                    .active()
                    .and_then(|d| d.log.as_deref()?.commits.get(row).map(|c| c.author.clone()));
                if let Some(author) = author {
                    self.change_log_query(|q| {
                        q.authors = vec![author];
                    });
                }
            }
            LogTarget::DetailsFilterAuthor => {
                let author = self.editor.active().and_then(|d| {
                    let v = d.log.as_deref()?;
                    v.commits.get(v.selected?).map(|c| c.author.clone())
                });
                if let Some(author) = author {
                    self.change_log_query(|q| {
                        q.authors = vec![author];
                    });
                }
            }
            LogTarget::RowCopyHash(row) => {
                let oid = self
                    .editor
                    .active()
                    .and_then(|d| d.log.as_deref()?.commits.get(row).map(|c| c.oid.clone()));
                if let Some(oid) = oid {
                    self.flash(t!("git-log-copied-oid", commit = short(&oid)));
                    self.set_clipboard(oid, false);
                }
            }
            LogTarget::RowResetSoft(row) => {
                let oid = self
                    .editor
                    .active()
                    .and_then(|d| d.log.as_deref()?.commits.get(row).map(|c| c.oid.clone()));
                if let Some(oid) = oid {
                    self.confirm_log(LogConfirmed::ResetSoft(oid));
                }
            }
            LogTarget::RowResetHard(row) => {
                let oid = self
                    .editor
                    .active()
                    .and_then(|d| d.log.as_deref()?.commits.get(row).map(|c| c.oid.clone()));
                if let Some(oid) = oid {
                    self.confirm_log(LogConfirmed::ResetHard(oid));
                }
            }
            LogTarget::RowNewTag(row) => {
                let oid = self
                    .editor
                    .active()
                    .and_then(|d| d.log.as_deref()?.commits.get(row).map(|c| c.oid.clone()));
                if let Some(oid) = oid {
                    self.open_tag_prompt(oid);
                }
            }
            LogTarget::RowNewBranch(row) => {
                let oid = self
                    .editor
                    .active()
                    .and_then(|d| d.log.as_deref()?.commits.get(row).map(|c| c.oid.clone()));
                if let Some(oid) = oid {
                    self.open_branch_dialog(None, Some(oid));
                }
            }
            LogTarget::RowMore(row) => {
                let oid = self
                    .editor
                    .active()
                    .and_then(|d| d.log.as_deref()?.commits.get(row).map(|c| c.oid.clone()));
                let hit = self.hits.log.iter().find(|(_, t)| *t == target).copied();
                if let (Some(oid), Some((rect, _))) = (oid, hit) {
                    self.open_context_menu(
                        MenuTarget::LogCommit(oid),
                        (rect.x, rect.bottom() + 4.0),
                    );
                }
            }
            LogTarget::RefCheckout(row, ref_idx) => {
                let reference = self.editor.active().and_then(|d| {
                    d.log
                        .as_deref()?
                        .commits
                        .get(row)?
                        .refs
                        .get(ref_idx)
                        .cloned()
                });
                if let Some(Ref::Branch(name)) = reference {
                    self.confirm_log(LogConfirmed::CheckoutBranch(name));
                }
            }
            LogTarget::RefRemove(row, ref_idx) => {
                let reference = self.editor.active().and_then(|d| {
                    d.log
                        .as_deref()?
                        .commits
                        .get(row)?
                        .refs
                        .get(ref_idx)
                        .cloned()
                });
                match reference {
                    Some(Ref::Branch(name)) => {
                        self.confirm_delete_branch(name);
                    }
                    Some(Ref::Remote(name)) => {
                        self.confirm_log(LogConfirmed::DeleteRemoteBranch(name));
                    }
                    Some(Ref::Tag(name)) => {
                        self.confirm_log(LogConfirmed::DeleteTag(
                            name,
                            winc_editor::git::TagTarget::Local,
                        ));
                    }
                    _ => {}
                }
            }
            LogTarget::CopyCommitText => {
                let oid = self
                    .editor
                    .active()
                    .and_then(|d| d.log.as_deref()?.selected_oid().map(str::to_owned));
                if let Some(oid) = oid {
                    self.run_log_commit_command(MenuCommand::LogCopyMessage, &oid);
                }
            }
            LogTarget::CloseDetails => {
                self.git.log.details_open = false;
                self.git.log.file = None;
                if self.focus == Focus::LogFileFilter {
                    self.focus = Focus::Editor;
                }
                self.clear_log_compare();
            }
            LogTarget::LayoutToggle => {
                self.git.log.side_by_side = !self.git.log.side_by_side;
            }
            LogTarget::SearchButton => {
                self.focus = Focus::Editor;
                self.log_filter_changed();
            }
            LogTarget::CompareSwap => self.swap_log_compare(),
            LogTarget::CompareExit => self.clear_log_compare(),
            // 拖动由 `mouse.rs` 开始处理。
            LogTarget::Divider => {}
            LogTarget::Line(row, line) => {
                self.focus = Focus::Editor;
                match self.log_line_target(row, line) {
                    Some(target) => {
                        self.clear_log_compare();
                        self.jump_log_row(target);
                    }
                    None => self.flash(t!("git-log-target-not-loaded")),
                }
            }
            LogTarget::Parent(i) => {
                let row = self.editor.active().and_then(|d| {
                    let view = d.log.as_deref()?;
                    let parent = view.commits.get(view.selected?)?.parents.get(i)?;
                    view.commits.iter().position(|c| &c.oid == parent)
                });
                match row {
                    Some(row) => self.jump_log_row(row),
                    None => self.flash(t!("git-log-parent-not-loaded")),
                }
            }
            LogTarget::CopyOid => {
                let oid = self
                    .editor
                    .active()
                    .and_then(|d| d.log.as_deref()?.selected_oid().map(str::to_owned));
                if let Some(oid) = oid {
                    self.flash(t!("git-log-copied-oid", commit = short(&oid)));
                    self.set_clipboard(oid, false);
                }
            }
            LogTarget::Refresh => {
                if let Some(id) = self.editor.active().map(|d| d.id) {
                    self.git.log.refreshing = Some(id);
                    self.reload_log(id);
                }
                // 列表上方的未提交更改和分支筛选来自仓库状态，因此同时刷新仓库状态。
                self.git_refresh();
            }
            LogTarget::JumpHead => {
                self.clear_log_compare();
                let head = self
                    .editor
                    .active()
                    .and_then(|d| d.log.as_deref()?.head_row());
                match head {
                    Some(row) => self.jump_log_row(row),
                    None => self.flash(t!("git-log-target-not-loaded")),
                }
            }
            LogTarget::Search => {
                self.focus = Focus::LogFilter;
                let (x, y) = self.pointer.mouse;
                self.click_field(x, y);
            }
            LogTarget::ClearSearch => {
                self.git.log.filter.set_text("");
                self.change_log_query(|q| q.text.clear());
            }
            LogTarget::Branch => self.open_branch_picker(crate::app::GitAnchor::Log),
            LogTarget::Author => {
                let Some(rect) = self
                    .hits
                    .log
                    .iter()
                    .find(|(_, t)| *t == target)
                    .map(|(r, _)| *r)
                else {
                    return;
                };
                self.git.log.authors = self.log_authors();
                self.open_context_menu(MenuTarget::LogAuthor, (rect.x, rect.bottom() + 4.0));
            }
            LogTarget::FilesLayout => self.toggle_commit_layout(),
            LogTarget::BranchesToggle => self.toggle_log_branches(),
            LogTarget::BranchFilter => self.press_log_branch_filter(),
            LogTarget::FileFilter => self.press_log_file_filter(),
            LogTarget::ClearFileFilter => self.clear_log_file_filter(),
            LogTarget::BranchRow(i) => self.press_log_branch_row(i),
            LogTarget::Date | LogTarget::Path => {
                let hit = self.hits.log.iter().find(|(_, t)| *t == target);
                let Some(rect) = hit.map(|(r, _)| *r) else {
                    return;
                };
                let menu = match target {
                    LogTarget::Date => MenuTarget::LogDate,
                    _ => {
                        self.git.log.paths = self.log_path_choices();
                        MenuTarget::LogPath
                    }
                };
                self.open_context_menu(menu, (rect.x, rect.bottom() + 4.0));
            }
            LogTarget::ClearFilters => {
                self.git.log.filter.set_text("");
                self.change_log_query(|q| {
                    q.text.clear();
                    q.branches.clear();
                    q.authors.clear();
                    q.pin = None;
                    q.since_days = None;
                    q.paths.clear();
                });
            }
        }
        self.request_redraw();
    }
}
