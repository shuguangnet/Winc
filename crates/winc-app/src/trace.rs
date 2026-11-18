//! 临时：把按键从窗口事件到伪终端的每一步写进一个日志文件，用于定位「终端里 Tab 不补全」。
//!
//! 这个模块只出现在诊断构建里，不进 `test-dmg`。日志文件在
//! `~/Library/Application Support/winc/key-trace.log`（macOS）或
//! `$XDG_CONFIG_HOME/winc/key-trace.log`（其他系统），每次启动覆盖。

use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// 日志文件的位置。
fn path() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let dir = if cfg!(target_os = "macos") {
        home.unwrap_or_default()
            .join("Library/Application Support/winc")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| home.map(|home| home.join(".config")))
            .unwrap_or_default()
            .join("winc")
    };
    dir.join("key-trace.log")
}

/// 写入日志的文件，第一次调用时打开。
#[allow(clippy::disallowed_methods)]
fn log() -> &'static Mutex<Option<std::fs::File>> {
    static LOG: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();
    LOG.get_or_init(|| {
        let file = std::fs::File::create(path()).ok();
        Mutex::new(file)
    })
}

/// 启动时刻，日志里的毫秒数从这里算起。
fn start() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}

/// 往日志里写一行：`<毫秒> <内容>`；写不进去时什么也不做（诊断构建不打扰用户）。
pub(crate) fn key(line: impl AsRef<str>) {
    let ms = start().elapsed().as_millis();
    let line = format!("{ms:>7} {}\n", line.as_ref());
    if let Ok(mut file) = log().lock()
        && let Some(file) = file.as_mut()
    {
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }
}
