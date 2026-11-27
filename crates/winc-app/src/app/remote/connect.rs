//! 连接远端主机的输入解析与 `~/.ssh/config` 写入辅助。
//!
//! 支持直接粘贴常见的 `ssh` 命令或 `ssh://` 地址并拆分为主机、用户名、端口与密钥，
//! 也可在连接时将常用主机一键追加到 `~/.ssh/config`。

use std::io;
use std::path::Path;

use winc_backend::Target;

/// 从粘贴的 `ssh` 命令、`ssh://` 地址或 `用户@主机:端口` 中解析出的连接字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedSsh {
    /// 纯主机名或 IP（不含 `用户@` 与 `:端口`）。
    pub host: String,
    /// 可选用户名。
    pub user: Option<String>,
    /// 可选端口（1..=65535）。
    pub port: Option<String>,
    /// 可选密钥文件路径。
    pub key: Option<String>,
    /// 可选远端目录（来自 `ssh://user@host/path`）。
    pub folder: Option<String>,
}

/// 把 `用户@主机` 拆成 `(Some(用户), 主机)`；不含 `@` 时返回 `(None, 原文)`。
pub(super) fn split_user_host(text: &str) -> (Option<&str>, &str) {
    match text.split_once('@') {
        Some((user, host)) if !user.is_empty() && !host.is_empty() => (Some(user), host),
        _ => (None, text),
    }
}

/// 按用户名和主机拼出传给 [`Target`] 的主机串：用户名非空时覆盖主机里已有的 `用户@` 前缀。
pub(super) fn combine_user_host(user: &str, host: &str) -> String {
    let user = user.trim();
    let host = host.trim();
    if user.is_empty() {
        return host.to_owned();
    }
    let bare = host.split_once('@').map_or(host, |(_, h)| h);
    if bare.is_empty() {
        String::new()
    } else {
        format!("{user}@{bare}")
    }
}

/// 校验独立用户名输入框的内容：为空合法，非空时只接受 SSH 目标允许的字符且不含 `@`、`:`、空白。
pub(super) fn valid_user(user: &str) -> bool {
    user.is_empty()
        || (!user.starts_with('-')
            && user
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')))
}

/// 尝试解析粘贴进来的完整 `ssh` 命令、`ssh://` 链接或 `主机:端口` 写法。
///
/// 普通手打的 `host` 或 `user@host` 返回 `None`，避免在用户输入途中干扰光标。
pub(super) fn parse_ssh_input(input: &str) -> Option<ParsedSsh> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed
        .strip_prefix("ssh://")
        .or_else(|| trimmed.strip_prefix("SSH://"))
    {
        return parse_ssh_url(rest);
    }
    let words = split_shell_words(trimmed);
    if let Some(first) = words.first()
        && first.eq_ignore_ascii_case("ssh")
        && words.len() >= 2
    {
        return parse_ssh_args(&words[1..]);
    }
    // `user@host:port` 或 `host:port`（必须是合法端口且不含空白）。
    if !trimmed.contains(char::is_whitespace)
        && let Some((left, port)) = trimmed.split_once(':')
        && is_valid_port(port)
        && !left.is_empty()
    {
        let (user, host) = split_user_host(left);
        if !host.is_empty() && Target::with_port(left, port.parse().ok()).is_some() {
            return Some(ParsedSsh {
                host: host.to_owned(),
                user: user.map(str::to_owned),
                port: Some(port.to_owned()),
                key: None,
                folder: None,
            });
        }
    }
    None
}

/// 解析 `ssh://[user@]host[:port][/folder]`。
fn parse_ssh_url(rest: &str) -> Option<ParsedSsh> {
    let (authority, folder) = match rest.find('/') {
        Some(at) => (&rest[..at], Some(rest[at..].to_owned())),
        None => (rest, None),
    };
    let (host_part, port) = match authority.rsplit_once(':') {
        Some((h, p)) if is_valid_port(p) => (h, Some(p.to_owned())),
        _ => (authority, None),
    };
    let (user, host) = split_user_host(host_part);
    if host.is_empty()
        || Target::with_port(host_part, port.as_deref().and_then(|p| p.parse().ok())).is_none()
    {
        return None;
    }
    Some(ParsedSsh {
        host: host.to_owned(),
        user: user.map(str::to_owned),
        port,
        key: None,
        folder: folder.filter(|f| !f.is_empty()),
    })
}

/// 解析 `ssh` 之后的命令行参数，提取 `-p`、`-l`、`-i` 和目标主机。
fn parse_ssh_args(args: &[String]) -> Option<ParsedSsh> {
    let mut user: Option<String> = None;
    let mut port: Option<String> = None;
    let mut key: Option<String> = None;
    let mut target_arg: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--" {
            if let Some(next) = args.get(i + 1) {
                target_arg = Some(next.as_str());
            }
            break;
        }
        if let Some(value) = arg.strip_prefix("-p") {
            let val = if value.is_empty() {
                i += 1;
                args.get(i).map(String::as_str)?
            } else {
                value
            };
            if is_valid_port(val) {
                port = Some(val.to_owned());
            }
        } else if let Some(value) = arg.strip_prefix("-l") {
            let val = if value.is_empty() {
                i += 1;
                args.get(i).map(String::as_str)?
            } else {
                value
            };
            if valid_user(val) && !val.is_empty() {
                user = Some(val.to_owned());
            }
        } else if let Some(value) = arg.strip_prefix("-i") {
            let val = if value.is_empty() {
                i += 1;
                args.get(i).map(String::as_str)?
            } else {
                value
            };
            if !val.is_empty() {
                key = Some(val.to_owned());
            }
        } else if arg.starts_with('-') && arg.len() == 2 {
            // 带参数的其他常见 ssh 选项（`-o`、`-J`、`-F`、`-L`、`-R`、`-D`、`-W` 等）跳过其值。
            let takes_value = matches!(
                arg.as_bytes()[1],
                b'B' | b'b'
                    | b'c'
                    | b'D'
                    | b'E'
                    | b'e'
                    | b'F'
                    | b'I'
                    | b'J'
                    | b'L'
                    | b'm'
                    | b'O'
                    | b'o'
                    | b'P'
                    | b'Q'
                    | b'R'
                    | b'S'
                    | b'W'
                    | b'w'
            );
            if takes_value {
                i += 1;
            }
        } else if !arg.starts_with('-') && target_arg.is_none() {
            target_arg = Some(arg);
        }
        i += 1;
    }
    let raw = target_arg?;
    if let Some(rest) = raw
        .strip_prefix("ssh://")
        .or_else(|| raw.strip_prefix("SSH://"))
    {
        let mut parsed = parse_ssh_url(rest)?;
        if parsed.user.is_none() {
            parsed.user = user;
        }
        if parsed.port.is_none() {
            parsed.port = port;
        }
        parsed.key = key;
        return Some(parsed);
    }
    let (host_part, inline_port) = match raw.rsplit_once(':') {
        Some((h, p)) if is_valid_port(p) => (h, Some(p.to_owned())),
        _ => (raw, None),
    };
    let (inline_user, host) = split_user_host(host_part);
    let final_user = inline_user.map(str::to_owned).or(user);
    let final_port = port.or(inline_port);
    let combined = combine_user_host(final_user.as_deref().unwrap_or(""), host);
    Target::with_port(
        &combined,
        final_port.as_deref().and_then(|p| p.parse().ok()),
    )?;
    Some(ParsedSsh {
        host: host.to_owned(),
        user: final_user,
        port: final_port,
        key,
        folder: None,
    })
}

/// 按简化 shell 规则拆分参数（支持单引号和双引号包裹含空格的路径）。
fn split_shell_words(input: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut in_word = false;
    for ch in input.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {
                current.push(ch);
                in_word = true;
            }
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                in_word = true;
            }
            None if ch.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut current));
                    in_word = false;
                }
            }
            None => {
                current.push(ch);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(current);
    }
    words
}

fn is_valid_port(port: &str) -> bool {
    port.parse::<u16>()
        .is_ok_and(|n| n != 0 && n.to_string() == port)
}

/// 把主机配置追加保存到 `path`（通常是 `~/.ssh/config`），返回写入或已存在的 `Host` 别名。
///
/// 若 `host_name` 已经是配置文件中的 `Host` 别名，则不重复写入，直接返回该别名。
#[allow(
    clippy::disallowed_methods,
    reason = "只访问本机：用户主目录中的 ~/.ssh/config"
)]
pub(super) fn save_host_to_ssh_config(
    path: &Path,
    host_name: &str,
    user: Option<&str>,
    port: Option<&str>,
    key: Option<&str>,
) -> io::Result<String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let known = super::config_hosts(&existing);
    if known.iter().any(|h| h == host_name) {
        return Ok(host_name.to_owned());
    }
    let alias = host_name.to_owned();
    let mut block = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') {
        block.push('\n');
    }
    if !existing.is_empty() {
        block.push('\n');
    }
    block.push_str(&format!("Host {alias}\n  HostName {host_name}\n"));
    if let Some(user) = user.filter(|u| !u.is_empty()) {
        block.push_str(&format!("  User {user}\n"));
    }
    if let Some(port) = port.filter(|p| !p.is_empty()) {
        block.push_str(&format!("  Port {port}\n"));
    }
    if let Some(key) = key.filter(|k| !k.is_empty()) {
        if key.contains(char::is_whitespace) {
            block.push_str(&format!("  IdentityFile \"{key}\"\n"));
        } else {
            block.push_str(&format!("  IdentityFile {key}\n"));
        }
    }
    #[cfg(unix)]
    let newly_created = !path.exists();
    let mut updated = existing;
    updated.push_str(&block);
    std::fs::write(path, updated)?;
    #[cfg(unix)]
    if newly_created {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(alias)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ssh_commands_urls_and_host_port() {
        assert_eq!(
            parse_ssh_input("ssh shuguang@192.168.1.10 -p 2222 -i ~/.ssh/id_ed25519"),
            Some(ParsedSsh {
                host: "192.168.1.10".into(),
                user: Some("shuguang".into()),
                port: Some("2222".into()),
                key: Some("~/.ssh/id_ed25519".into()),
                folder: None,
            })
        );
        assert_eq!(
            parse_ssh_input("ssh -A -l shuguang -p2200 -i \"~/.ssh/my key\" macmini.local"),
            Some(ParsedSsh {
                host: "macmini.local".into(),
                user: Some("shuguang".into()),
                port: Some("2200".into()),
                key: Some("~/.ssh/my key".into()),
                folder: None,
            })
        );
        assert_eq!(
            parse_ssh_input("ssh://shuguang@192.168.1.10:2222/Users/shuguang/project"),
            Some(ParsedSsh {
                host: "192.168.1.10".into(),
                user: Some("shuguang".into()),
                port: Some("2222".into()),
                key: None,
                folder: Some("/Users/shuguang/project".into()),
            })
        );
        assert_eq!(
            parse_ssh_input("shuguang@192.168.1.10:2222"),
            Some(ParsedSsh {
                host: "192.168.1.10".into(),
                user: Some("shuguang".into()),
                port: Some("2222".into()),
                key: None,
                folder: None,
            })
        );
        assert_eq!(parse_ssh_input("macmini"), None);
        assert_eq!(parse_ssh_input("shuguang@macmini"), None);
    }

    #[test]
    fn saves_new_host_to_ssh_config_without_duplicates() {
        let dir = std::env::temp_dir().join(format!("winc-ssh-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = dir.join("config");

        let alias = save_host_to_ssh_config(
            &config,
            "192.168.1.10",
            Some("shuguang"),
            Some("2222"),
            Some("~/.ssh/my key"),
        )
        .unwrap();
        assert_eq!(alias, "192.168.1.10");
        let text = std::fs::read_to_string(&config).unwrap();
        assert!(text.contains("Host 192.168.1.10\n  HostName 192.168.1.10\n"));
        assert!(text.contains("  User shuguang\n"));
        assert!(text.contains("  Port 2222\n"));
        assert!(text.contains("  IdentityFile \"~/.ssh/my key\"\n"));

        // 再次保存同一主机时不重复追加。
        let again =
            save_host_to_ssh_config(&config, "192.168.1.10", Some("shuguang"), None, None).unwrap();
        assert_eq!(again, "192.168.1.10");
        assert_eq!(std::fs::read_to_string(&config).unwrap(), text);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
