//! 构建随安装包发布的远端服务进程（`winc-server`）。
//!
//! Linux 架构的服务进程静态链接 musl，不依赖远端的系统库；没有 C 代码，用 Rust 自带的链接器
//! 即可在任何系统上构建。在 macOS 上构建时同时编译 macOS（`Darwin-arm64` / `Darwin-x86_64`）
//! 架构的服务进程，使安装包也支持通过 SSH 连接 macOS 远端主机。

use std::path::PathBuf;
use std::process::Command;

use crate::{root, run};

/// Linux 架构的构建目标，以及程序在安装包中的文件名。
///
/// 文件名的后缀是该系统上 `uname -s` 与 `uname -m` 的输出以 `-` 相连，连接时按它选取。
pub const PROGRAMS: [(&str, &str); 2] = [
    ("x86_64-unknown-linux-musl", "winc-server-Linux-x86_64"),
    ("aarch64-unknown-linux-musl", "winc-server-Linux-aarch64"),
];

/// macOS 架构的构建目标（在 macOS 上用系统 SDK 构建），以及程序在安装包中的文件名。
pub const MACOS_PROGRAMS: [(&str, &str); 2] = [
    ("aarch64-apple-darwin", "winc-server-Darwin-arm64"),
    ("x86_64-apple-darwin", "winc-server-Darwin-x86_64"),
];

/// 构建好的程序所在的目录，打包脚本从这里取用。
pub fn dir() -> PathBuf {
    root().join("target/remote-server")
}

/// 构建各个架构的服务进程，放进 [`dir`]，返回该目录。
///
/// # Errors
///
/// 无法安装构建目标、构建失败或复制失败时返回错误描述。
pub fn build() -> Result<PathBuf, String> {
    let out = dir();
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    for (target, name) in PROGRAMS {
        run(Command::new("rustup").args(["target", "add", target]))?;
        let linker = format!(
            "CARGO_TARGET_{}_LINKER",
            target.to_uppercase().replace('-', "_")
        );
        run(Command::new(env!("CARGO"))
            .args(["build", "--release", "--locked", "-p", "winc-server"])
            .args(["--target", target])
            .env(linker, "rust-lld")
            .current_dir(root()))?;
        let built = root()
            .join("target")
            .join(target)
            .join("release/winc-server");
        std::fs::copy(&built, out.join(name))
            .map_err(|e| format!("无法复制 {}：{e}", built.display()))?;
    }
    #[cfg(target_os = "macos")]
    for (target, name) in MACOS_PROGRAMS {
        run(Command::new("rustup").args(["target", "add", target]))?;
        run(Command::new(env!("CARGO"))
            .args(["build", "--release", "--locked", "-p", "winc-server"])
            .args(["--target", target])
            .env("MACOSX_DEPLOYMENT_TARGET", "11.0")
            .current_dir(root()))?;
        let built = root()
            .join("target")
            .join(target)
            .join("release/winc-server");
        let dest = out.join(name);
        std::fs::copy(&built, &dest).map_err(|e| format!("无法复制 {}：{e}", built.display()))?;
        // Apple Silicon 要求 Mach-O 可执行文件至少带有临时签名，否则上传到远端 macOS 后无法执行。
        run(Command::new("codesign")
            .args(["--force", "--sign", "-"])
            .arg(&dest))?;
    }
    Ok(out)
}
