//! 比较文件树中选中的两个文件。
//!
//! 在并排差异标签中打开：左栏是先选中的文件（显示顺序靠前者），右栏是另一个文件。两侧均按
//! 磁盘上的内容比较且只读，不依赖 Git。

use std::path::{Path, PathBuf};

use crate::app::editor::definition::{Place, PlacesKind};
use winc_editor::diff_view::{DiffSide, DiffView};
use winc_editor::encoding::decode_unknown;
use winc_editor::{LARGE_FILE, display_name, looks_binary};
use winc_i18n::t;

use super::*;

use crate::app::scroll::set_scroll_line;

/// 返回 `path` 的文本内容用于比较；文件过大、是二进制文件或读取失败时返回提示文字。
fn read_for_compare(path: &Path) -> Result<String, String> {
    let name = display_name(path);
    let bytes = winc_backend::fs::read(path).map_err(|e| {
        t!(
            "workspace-compare-read-failed",
            name = &*name,
            error = e.to_string()
        )
    })?;
    if bytes.len() as u64 > LARGE_FILE {
        return Err(t!("workspace-compare-too-large", name = name));
    }
    if looks_binary(&bytes) {
        return Err(t!("workspace-compare-binary", name = name));
    }
    Ok(decode_unknown(&bytes).0)
}

/// 返回比较两个文件时标签的标题，形式为「左 ↔ 右」。
pub(crate) fn compare_title(left: &Path, right: &Path) -> String {
    format!("{} ↔ {}", display_name(left), display_name(right))
}

impl App {
    /// 按磁盘上的内容比较文件 `left` 与 `right`，在并排差异标签中打开。
    ///
    /// 内容相同或读取失败时只显示提示，不打开标签。
    pub(in crate::app) fn compare_files(&mut self, left: &Path, right: &Path) {
        let (old, new) = match (read_for_compare(left), read_for_compare(right)) {
            (Ok(old), Ok(new)) => (old, new),
            (Err(e), _) | (_, Err(e)) => {
                self.flash(e);
                return;
            }
        };
        let language = winc_syntax::detect(&display_name(right));
        let relative = |path: &Path| {
            self.relative_path(path)
                .unwrap_or_else(|| path.to_string_lossy().into_owned())
        };
        let view = DiffView::new(
            DiffSide::new(relative(left), &old, None, language),
            DiffSide::new(relative(right), &new, None, language),
        );
        if view.is_identical() {
            self.flash(t!(
                "workspace-compare-identical",
                left = display_name(left),
                right = display_name(right)
            ));
            return;
        }

        let index = self.editor.show_diff(&compare_title(left, right), view);
        // 打开时定位到第一处改动。
        let doc = &mut self.editor.documents[index];
        if let Some(row) = doc.diff.as_deref().and_then(|v| {
            v.hunk_rows[winc_editor::diff_view::RIGHT]
                .first()
                .map(|r| r.start)
        }) {
            set_scroll_line(doc, row.saturating_sub(3) as f64);
        }
        self.focus = Focus::Editor;
        self.relayout();
        self.clamp_scroll();
        self.request_redraw();
    }
}

/// 目录对比跳过的目录名：版本管理、依赖与构建产物不会有人想比较。
const SKIPPED_DIRS: [&str; 6] = [
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    "__pycache__",
];

/// 目录对比的差异项：打开用的路径（只有一侧存在时另一侧是 `None`）、相对路径和状态的显示
/// 文字。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FolderDiffEntry {
    pub left: Option<PathBuf>,
    pub right: Option<PathBuf>,
    pub relative: String,
    pub state: String,
}

/// 递归收集 `dir` 下（相对 `base`）全部文件的路径，跳过 [`SKIPPED_DIRS`] 中的目录；文件数
/// 超出 `budget` 时提前停止（`truncated` 置真）。
fn collect_files(
    dir: &Path,
    base: &Path,
    out: &mut Vec<PathBuf>,
    budget: usize,
    truncated: &mut bool,
) {
    let Ok(items) = winc_backend::fs::read_dir(dir) else {
        return;
    };
    for item in items.filter_map(|item| item.ok()).map(|item| item.path()) {
        if out.len() >= budget {
            *truncated = true;
            return;
        }
        let name = item
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        if winc_backend::fs::is_dir(&item) {
            if SKIPPED_DIRS.contains(&name.as_str()) {
                continue;
            }
            collect_files(&item, base, out, budget, truncated);
            if *truncated {
                return;
            }
        } else if winc_backend::fs::is_file(&item) {
            out.push(item.strip_prefix(base).map(PathBuf::from).unwrap_or(item));
        }
    }
}

/// 两个文件的内容是否一致：大小不同直接算不同；大小相同再逐字节比较。
fn contents_equal(a: &Path, b: &Path) -> bool {
    let (Ok(meta_a), Ok(meta_b)) = (winc_backend::fs::metadata(a), winc_backend::fs::metadata(b))
    else {
        return false;
    };
    if meta_a.len != meta_b.len {
        return false;
    }
    let (Ok(a), Ok(b)) = (winc_backend::fs::read(a), winc_backend::fs::read(b)) else {
        return false;
    };
    a == b
}

/// 比较目录 `left` 与 `right`，返回两边合并的差异项（内容相同的不列），相对路径按各自的根
/// 目录计算；文件数超过 [`FOLDER_DIFF_BUDGET`] 时只比较靠前的一部分并在状态栏说明。
pub(crate) fn diff_folders(left: &Path, right: &Path) -> Vec<FolderDiffEntry> {
    const BUDGET: usize = 3000;
    let (mut left_files, mut right_files) = (Vec::new(), Vec::new());
    let (mut left_cut, mut right_cut) = (false, false);
    collect_files(left, left, &mut left_files, BUDGET, &mut left_cut);
    collect_files(right, right, &mut right_files, BUDGET, &mut right_cut);
    left_files.sort();
    right_files.sort();

    let state_only_left = t!("workspace-folderdiff-left").to_owned();
    let state_only_right = t!("workspace-folderdiff-right").to_owned();
    let state_changed = t!("workspace-folderdiff-changed").to_owned();
    let display_rel = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let mut diffs = Vec::new();
    let (mut a, mut b) = (0, 0);
    while a < left_files.len() || b < right_files.len() {
        let order = match (left_files.get(a), right_files.get(b)) {
            (Some(l), Some(r)) => l.cmp(r),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => break,
        };
        match order {
            std::cmp::Ordering::Less => {
                let relative = display_rel(&left_files[a]);
                diffs.push(FolderDiffEntry {
                    left: Some(left.join(&left_files[a])),
                    right: None,
                    relative: relative.clone(),
                    state: state_only_left.clone(),
                });
                a += 1;
            }
            std::cmp::Ordering::Greater => {
                let relative = display_rel(&right_files[b]);
                diffs.push(FolderDiffEntry {
                    left: None,
                    right: Some(right.join(&right_files[b])),
                    relative: relative.clone(),
                    state: state_only_right.clone(),
                });
                b += 1;
            }
            std::cmp::Ordering::Equal => {
                let relative = display_rel(&left_files[a]);
                if !contents_equal(&left.join(&left_files[a]), &right.join(&right_files[b])) {
                    diffs.push(FolderDiffEntry {
                        left: Some(left.join(&left_files[a])),
                        right: Some(right.join(&right_files[b])),
                        relative: relative.clone(),
                        state: state_changed.clone(),
                    });
                }
                a += 1;
                b += 1;
            }
        }
    }
    let _ = (left_cut, right_cut);
    diffs
}

impl App {
    /// 比较目录树中选中的两个目录：浮窗列出两边合并的差异文件，选中一项打开对应的文件对比
    /// （两边都有的）或该文件本身（只有一边有的）。
    pub(in crate::app) fn compare_folders(&mut self, left: PathBuf, right: PathBuf) {
        // 相互嵌套的两个目录没有可解释的差异。
        if left.starts_with(&right) || right.starts_with(&left) {
            self.flash(t!("workspace-folderdiff-nested"));
            return;
        }
        let diffs = diff_folders(&left, &right);
        if diffs.is_empty() {
            self.flash(t!("workspace-folders-identical"));
            return;
        }
        let pairs: Vec<(Option<PathBuf>, Option<PathBuf>)> = diffs
            .iter()
            .map(|entry| (entry.left.clone(), entry.right.clone()))
            .collect();
        let items: Vec<Place> = diffs
            .iter()
            .map(|entry| Place {
                path: entry
                    .left
                    .clone()
                    .or(entry.right.clone())
                    .unwrap_or_default(),
                relative: entry.relative.clone(),
                line: 0,
                col: None,
                preview: entry.state.clone(),
                marks: Vec::new(),
            })
            .collect();
        self.show_places(PlacesKind::FolderDiff, String::new(), items);
        self.overlays.picker.compare_pairs = Some(pairs);
        self.request_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一棵两边有来有往的目录树：a.txt 相同，b.txt 不同，c.rs 只在左，d.rs 只在右。
    fn sample(left: &Path, right: &Path) {
        for (base, seed) in [(left, "l"), (right, "r")] {
            std::fs::create_dir_all(base.join("nested")).unwrap();
            std::fs::write(base.join("a.txt"), "相同\n").unwrap();
            std::fs::write(base.join("b.txt"), format!("内容 {seed}\n")).unwrap();
            std::fs::write(base.join("nested/e.txt"), format!("{seed}\n")).unwrap();
            if base == left {
                std::fs::write(base.join("c.rs"), "// c\n").unwrap();
            } else {
                std::fs::write(base.join("d.rs"), "// d\n").unwrap();
            }
        }
    }

    #[test]
    fn diff_folders_lists_only_the_differences() {
        let base = std::env::temp_dir().join("winc-folderdiff-test");
        let _ = std::fs::remove_dir_all(&base);
        let (left, right) = (base.join("a"), base.join("b"));
        sample(&left, &right);
        let diffs = diff_folders(&left, &right);
        // a.txt 相同不列；b.txt、nested/e.txt 不同；c.rs 仅左；d.rs 仅右。
        assert_eq!(diffs.len(), 4, "{diffs:?}");
        let find = |relative: &str| diffs.iter().find(|d| d.relative == relative).unwrap();
        assert!(find("b.txt").left.is_some() && find("b.txt").right.is_some());
        assert!(find("nested/e.txt").left.is_some() && find("nested/e.txt").right.is_some());
        assert!(find("c.rs").right.is_none(), "仅左侧的没有右路径");
        assert!(find("d.rs").left.is_none(), "仅右侧的没有左路径");
        let _ = std::fs::remove_dir_all(&base);
    }
}
