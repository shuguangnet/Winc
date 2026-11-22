//! 本地历史：每次保存把文件内容快照到用户数据目录，随时可以回到任一保存点。
//!
//! 快照按「文件路径的散列」分目录存放（`history/<散列>/<毫秒时间戳>`），与项目目录无关，
//! 不会混进版本管理；目录里另有一份 `path` 记录原路径。每个文件最多保留 [`KEEP`] 份，超出
//! 时删除最旧的。写入在后台线程进行（[`Rope`] 的克隆是常数时间，序列化也不在界面线程）。
//!
//! 在编辑区的右键菜单选「本地历史」浏览：浮窗按时间从新到旧列出保存点，选中一项后打开它与
//! 当前内容的并排差异。

#![allow(
    clippy::disallowed_methods,
    reason = "本地历史只访问本机的用户数据目录（远端主机上的文件不记录），不经 winc_backend::fs"
)]

use std::path::{Path, PathBuf};

use winc_core::hash::Fnv;
use winc_editor::diff_view::{DiffSide, DiffView};
use winc_editor::encoding::decode_unknown;
use winc_editor::{LARGE_FILE, looks_binary};
use winc_i18n::t;

use crate::app::editor::definition::{Place, PlacesKind};
use crate::app::git::log::Loaded;
use crate::app::{App, DocumentId, UserEvent};

/// 每个文件保留多少份快照（份）：一天高频保存也就几十份，一百份覆盖数周的改动。
const KEEP: usize = 100;

/// 快照文件名的宽度：Unix 毫秒时间戳写成 19 位，按名字典序排列就是时间顺序。
const STAMP_LEN: usize = 19;

impl App {
    /// 把刚保存的文档 `id` 记入本地历史：后台写出快照并清理超额的旧快照。
    ///
    /// 远端主机上的文件、没有保存到磁盘的文档和大文件（超出 [`LARGE_FILE`]，与差异查看的
    /// 上限一致）不记录；设置 `localHistory` 为 `false` 时整体关闭。
    pub(in crate::app) fn record_local_history(&mut self, id: DocumentId) {
        if self.config.local_history == Some(false) {
            return;
        }
        let Some(doc) = self.editor.document(id) else {
            return;
        };
        let Some(path) = doc.path.clone() else {
            return;
        };
        if winc_backend::is_remote(&path) {
            return;
        }
        let Some(rope) = doc
            .text()
            .rope()
            .filter(|r| r.len_bytes() as u64 <= LARGE_FILE)
        else {
            return;
        };
        let text = rope.to_string();
        winc_editor::background::spawn_blocking(move || record(&path, &text));
    }

    /// 列出当前文档的本地历史：浮窗按时间从新到旧列出各次保存，选中一项打开它与当前内容的
    /// 差异。没有历史时在状态栏说明。
    pub(in crate::app) fn list_local_history(&mut self) {
        let Some(doc) = self.editor.active() else {
            return;
        };
        let Some(path) = doc.path.clone() else {
            self.flash(t!("editor-history-none"));
            return;
        };
        if winc_backend::is_remote(&path) {
            self.flash(t!("editor-history-none"));
            return;
        }
        let entries = entries(&path);
        if entries.is_empty() {
            self.flash(t!("editor-history-none"));
            return;
        }
        let files: Vec<PathBuf> = entries.iter().map(|entry| entry.file.clone()).collect();
        let items: Vec<Place> = entries
            .into_iter()
            .map(|entry| Place {
                path: path.clone(),
                relative: entry.time,
                line: 0,
                col: None,
                preview: String::new(),
                marks: Vec::new(),
            })
            .collect();
        self.show_places(PlacesKind::LocalHistory, String::new(), items);
        self.overlays.picker.history_files = Some(files);
        self.request_redraw();
    }

    /// 打开浮窗中选中的那项：历史快照 `snapshot` 与它所属文件 `file` 的当前内容的并排差异，
    /// `time` 是浮窗里显示的时间写法（放在差异的左侧标题上）。
    pub(in crate::app) fn open_history_snapshot(
        &mut self,
        snapshot: PathBuf,
        file: PathBuf,
        time: String,
    ) {
        let name = winc_editor::display_name(&file).into_owned();
        let title = t!("editor-history-tab-title", name = name.as_str());
        let sender = self.git.log.results.0.clone();
        self.spawn_blocking_then(UserEvent::GitLog, move || {
            let view = read_history_diff(&snapshot, &file, &name, &time);
            let _ = sender.send(Loaded::LocalDiff { title, view });
        });
    }
}

/// 一个保存点：时间写法（浮窗中显示）和快照文件的路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    /// 时间写法，浮窗中显示。
    pub time: String,
    /// 快照文件的路径。
    pub file: PathBuf,
}

/// 返回文件 `path` 的各份快照，时间从新到旧；读取目录失败时返回空。
fn entries(path: &Path) -> Vec<Entry> {
    let Some(base) = base() else {
        return Vec::new();
    };
    entries_in(&base, path)
}

/// 把 `content` 写成文件 `path` 的一份快照，并删除超出 [`KEEP`] 份的最旧快照。
fn record(path: &Path, content: &str) {
    let Some(base) = base() else {
        return;
    };
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    record_at(&base, path, content, millis);
}

/// 返回用户数据目录中的历史根目录（`history/`）；目录不存在时创建。
fn base() -> Option<PathBuf> {
    let dir = winc_config::state_path()?.parent()?.join("history");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// 返回文件 `path` 的历史目录：`base` 下按路径散列的一层；目录不存在时创建。
///
/// 散列用 FNV-1a（每次运行、每台机器上都相同，见 `winc_core::hash`），目录里的 `path`
/// 文件记录原路径，供人工辨认。
fn dir_in(base: &Path, path: &Path) -> Option<PathBuf> {
    let mut hash = Fnv::new();
    hash.write(path.to_string_lossy().as_bytes());
    let dir = base.join(format!("{:016x}", hash.finish()));
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// [`entries`] 的可测版本：历史根目录由 `base` 给出。
fn entries_in(base: &Path, path: &Path) -> Vec<Entry> {
    let Some(dir) = dir_in(base, path) else {
        return Vec::new();
    };
    let Ok(items) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut stamps: Vec<(String, PathBuf)> = items
        .filter_map(|item| item.ok())
        .map(|item| item.path())
        .filter(|file| is_snapshot(file))
        .map(|file| {
            let name = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            (format_stamp(name), file)
        })
        .collect();
    stamps.sort_by(|a, b| b.0.cmp(&a.0));
    stamps
        .into_iter()
        .map(|(time, file)| Entry { time, file })
        .collect()
}

/// [`record`] 的可测版本：历史根目录与时间戳由调用方给出。
fn record_at(base: &Path, path: &Path, content: &str, millis: i64) {
    let Some(dir) = dir_in(base, path) else {
        return;
    };
    // 原路径只记一次：已有的记录保持不变。
    let marked = dir.join("path");
    if !winc_backend::fs::exists(&marked) {
        let _ = std::fs::write(&marked, path.to_string_lossy().as_bytes());
    }
    let _ = std::fs::write(dir.join(format!("{millis:019}")), content);
    prune(&dir);
}

/// `file` 是否是一份快照：普通文件，名字是 [`STAMP_LEN`] 位的数字。
fn is_snapshot(file: &Path) -> bool {
    winc_backend::fs::is_file(file)
        && file
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.len() == STAMP_LEN && name.bytes().all(|b| b.is_ascii_digit()))
}

/// 删除目录 `dir` 里超出 [`KEEP`] 份的最旧快照（按文件名的字典序，即时间顺序）。
fn prune(dir: &Path) {
    let Ok(items) = std::fs::read_dir(dir) else {
        return;
    };
    let mut stamps: Vec<PathBuf> = items
        .filter_map(|item| item.ok())
        .map(|item| item.path())
        .filter(|file| is_snapshot(file))
        .collect();
    if stamps.len() <= KEEP {
        return;
    }
    stamps.sort();
    let extra = stamps.len() - KEEP;
    for file in &stamps[..extra] {
        let _ = std::fs::remove_file(file);
    }
}

/// 把快照文件名（Unix 毫秒，19 位）换算成本地时间的写法「2026-11-19 14:32:05」。
///
/// 换算不出（本机时区读不到）时原样返回文件名。
fn format_stamp(name: &str) -> String {
    let Ok(millis) = name.parse::<i64>() else {
        return name.to_owned();
    };
    let (y, m, d, h, min, s) = civil_local(millis.div_euclid(1000));
    format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02}:{s:02}")
}

/// 返回 Unix 秒 `time` 在本机时区的（年，月，日，时，分，秒）。
///
/// 本机时区与 UTC 的差按平台取：Unix 上 `localtime_r` 直接给出；Windows 上用时区偏差换算。
/// 读不到时按 UTC 处理，快照显示的时间可能差几个小时，快照本身不受影响。
fn civil_local(time: i64) -> (i64, i64, i64, i64, i64, i64) {
    let offset = local_offset(time);
    let local = time + offset;
    let days = local.div_euclid(86_400);
    let rest = local.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    (y, m, d, rest / 3600, rest % 3600 / 60, rest % 60)
}

#[allow(unsafe_code)]
fn local_offset(time: i64) -> i64 {
    #[cfg(unix)]
    {
        let now = time as libc::time_t;
        let mut tm = std::mem::MaybeUninit::<libc::tm>::zeroed();
        // SAFETY: `localtime_r` 只读 `now`、只写传入的结构体，失败时返回空指针。
        let filled = unsafe { !libc::localtime_r(&now, tm.as_mut_ptr()).is_null() };
        if filled {
            // SAFETY: 调用成功时结构体已填好。
            let tm = unsafe { tm.assume_init() };
            return tm.tm_gmtoff;
        }
        0
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        let _ = time;
        // SAFETY: `TIME_ZONE_INFORMATION` 是纯 C 结构体，全零位模式有效；
        // `GetTimeZoneInformation` 只写传入的结构体；返回值是时区的状态（未知、标准或夏令时），
        // 返回 -1（失败）时按标准偏差处理。
        let mut tz: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
        let state = unsafe { GetTimeZoneInformation(&mut tz) };
        let mut bias = tz.Bias;
        if state == 2 {
            bias += tz.DaylightBias;
        }
        -i64::from(bias) * 60
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = time;
        0
    }
}

/// 公历日期是 1970-01-01 之后第几天换回（年，月，日）（Howard Hinnant 的算法，与 Git 日志
/// 的时间显示共用同一套换算）。
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// 读出快照与当前内容，组成并排差异：左边是快照（标题是它的时间写法 `time`），右边是当前
/// 内容；当前文件已删除时右侧标明。内容太大或是二进制时说明原因。
fn read_history_diff(
    snapshot: &Path,
    file: &Path,
    name: &str,
    time: &str,
) -> Result<Box<DiffView>, String> {
    let failed = |e: String| t!("editor-history-read-failed", error = e);
    let old_bytes = std::fs::read(snapshot).map_err(|e| failed(e.to_string()))?;
    let new_bytes = std::fs::read(file);
    for bytes in std::iter::once(&old_bytes).chain(new_bytes.as_ref()) {
        if bytes.len() as u64 > LARGE_FILE {
            return Err(t!("git-diff-too-large", name = name));
        }
        if looks_binary(bytes) {
            return Err(t!("git-diff-binary", name = name));
        }
    }
    let language = winc_syntax::detect(name);
    let old_side = DiffSide::new(
        t!("editor-history-side", time = time),
        &decode_unknown(&old_bytes).0,
        None,
        language,
    );
    let new_side = match new_bytes {
        Ok(new_bytes) => DiffSide::new(
            t!("git-diff-side-worktree"),
            &decode_unknown(&new_bytes).0,
            None,
            language,
        ),
        Err(_) => {
            DiffSide::new(t!("git-diff-side-worktree"), "", None, language).with_missing(true)
        }
    };
    let mut view = DiffView::new(old_side, new_side);
    // 右侧是磁盘上的当前内容：改动可以直接保存回去。
    view.snapshot_path = Some(file.to_path_buf());
    Ok(Box::new(view))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 在临时目录里写 `n` 份快照（时间戳各差一秒：文件名的字典序就是时间顺序）。
    fn written(base: &Path, n: usize) {
        let path = Path::new("/work/src/main.rs");
        for i in 0..n {
            record_at(
                base,
                path,
                &format!("第 {i} 份"),
                1_700_000_000_000 + i as i64 * 1000,
            );
        }
    }

    #[test]
    fn entries_are_listed_newest_first_with_their_times() {
        let base = std::env::temp_dir().join("winc-history-test-entries");
        let _ = std::fs::remove_dir_all(&base);
        written(&base, 3);
        let entries = entries_in(&base, Path::new("/work/src/main.rs"));
        assert_eq!(entries.len(), 3, "三份快照都在");
        // 最新的在前；时间写法是「年-月-日 时:分:秒」。
        assert!(entries[0].time > entries[1].time && entries[1].time > entries[2].time);
        assert_eq!(entries[2].time.len(), 19, "{}", entries[2].time);
        let content = std::fs::read_to_string(&entries[0].file).unwrap();
        assert_eq!(content, "第 2 份", "最新的快照是最后一次保存的内容");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn old_snapshots_are_pruned() {
        let base = std::env::temp_dir().join("winc-history-test-prune");
        let _ = std::fs::remove_dir_all(&base);
        written(&base, KEEP + 10);
        let entries = entries_in(&base, Path::new("/work/src/main.rs"));
        assert_eq!(entries.len(), KEEP, "只保留最近的一份份快照中的前若干份");
        // 删掉的是最旧的：剩下最新的那些。
        let content = std::fs::read_to_string(&entries.last().unwrap().file).unwrap();
        assert_ne!(content, "第 0 份");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn different_files_do_not_share_a_directory() {
        let base = std::env::temp_dir().join("winc-history-test-separate");
        let _ = std::fs::remove_dir_all(&base);
        record_at(&base, Path::new("/work/a.rs"), "a", 1_700_000_000_000);
        record_at(&base, Path::new("/work/b.rs"), "b", 1_700_000_000_000);
        assert_eq!(entries_in(&base, Path::new("/work/a.rs")).len(), 1);
        assert_eq!(entries_in(&base, Path::new("/work/b.rs")).len(), 1);
        let _ = std::fs::remove_dir_all(&base);
    }
}
