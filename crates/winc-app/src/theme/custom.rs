#![allow(
    clippy::disallowed_methods,
    reason = "自定义主题只访问本机的用户数据目录，不经 winc_backend::fs"
)]

//! 用户自定义主题：数据目录 `themes/` 下的 JSON 文件，启动时读入，在主题菜单中与内置主题
//! 一起列出、选用与持久化。
//!
//! 文件用 JSONC（可以带注释），键名与 [`Tokens`] 的字段对应：`dark`、`frame`、`card`、
//! `popup`、`text`、`textMuted`、`textDim`、`accent`、`accentFill`、`selection`、`gutter`（2
//! 色）、`error`、`warning`、`git`（4 色）、`syntax`（9 色）、`ansi`（16 色，缺省用一套中性
//! 配色）、`tintedTab`；颜色写成 `"#RRGGBB"`。`dark`、`frame`、`card`、`text`、`accent`、
//! `git`、`syntax` 必填，其余缺省由既有颜色推算。
//!
//! 主题一旦读入就固化为 `&'static`（引用泄漏在 [`OnceLock`] 中），与内置主题共用
//! [`ThemeEntry`]，配置、菜单和网格的既有代码无需区分。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use winc_config::Json;
use winc_render::Color;

use super::*;

/// 已读入的自定义主题，按文件名的字典序排列；启动后不再变化。
static CUSTOM: OnceLock<&'static [ThemeEntry]> = OnceLock::new();

/// 启动时读入数据目录 `themes/` 下的全部主题文件；重复调用没有效果。
pub fn load() {
    let _ = CUSTOM.set(read_all());
}

/// 已读入的自定义主题。
pub fn entries() -> &'static [ThemeEntry] {
    CUSTOM.get().copied().unwrap_or(&[])
}

/// 自定义主题的显示名：JSON 里的 `name`，没有时是 id。
pub fn name(id: &str) -> Option<&'static str> {
    NAMES
        .get()
        .copied()
        .unwrap_or(&[])
        .iter()
        .find(|(key, _)| *key == id)
        .map(|(_, name)| *name)
}

/// 显示名的对照表：id → JSON 里的 `name`，与 [`CUSTOM`] 同批固化。
static NAMES: OnceLock<&'static [(&'static str, &'static str)]> = OnceLock::new();

/// 返回数据目录中的主题目录 `themes/`；不存在时创建。
fn dir() -> Option<PathBuf> {
    let dir = winc_config::state_path()?.parent()?.join("themes");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// 读入目录中的全部主题文件，固化为 `&'static`。
fn read_all() -> &'static [ThemeEntry] {
    let Ok(items) = std::fs::read_dir(dir().as_deref().unwrap_or(Path::new("/"))) else {
        return &[];
    };
    let mut out: Vec<(ThemeEntry, &'static str)> = Vec::new();
    for file in items.filter_map(|item| item.ok()).map(|item| item.path()) {
        let Some(id) = file.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if file.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let Some((name, palette)) = parse(id, &text) else {
            continue;
        };
        let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
        let name: &'static str = Box::leak(name.into_boxed_str());
        let palette: &'static Palette = Box::leak(Box::new(palette));
        out.push((ThemeEntry { id, palette }, name));
    }
    out.sort_by(|a, b| a.1.cmp(b.1));
    let (entries, names): (Vec<ThemeEntry>, Vec<(&'static str, &'static str)>) = out
        .into_iter()
        .map(|(entry, name)| {
            let id = entry.id;
            (entry, (id, name))
        })
        .unzip();
    NAMES.set(Box::leak(names.into_boxed_slice())).ok();
    Box::leak(entries.into_boxed_slice())
}

/// 把主题文件的内容解析成（显示名，配色）；`dark`、`frame`、`card`、`text`、`accent`、`git`、
/// `syntax` 缺一不可，其余缺省由既有颜色推算。
fn parse(id: &str, text: &str) -> Option<(String, Palette)> {
    let json = Json::parse_jsonc(text)?.0.as_object()?.clone();
    let dark = json.get("dark")?.as_bool()?;
    let color = |key: &str| json.get(key).and_then(Json::as_str).and_then(hex);
    let colors = |key: &str, count: usize| -> Option<Vec<Color>> {
        let list = json.get(key)?.as_array()?;
        let parsed: Option<Vec<Color>> = list
            .iter()
            .map(|value| value.as_str().and_then(hex))
            .collect();
        parsed.filter(|list| list.len() == count)
    };
    let text = color("text")?;
    let card = color("card")?;
    let accent = color("accent")?;
    let text_muted = mix(text, card, 0.45);
    let tokens = Tokens {
        dark,
        frame: color("frame")?,
        popup: json
            .get("popup")
            .and_then(Json::as_str)
            .and_then(hex)
            .unwrap_or(card),
        card,
        text,
        text_muted: json
            .get("textMuted")
            .and_then(Json::as_str)
            .and_then(hex)
            .unwrap_or(text_muted),
        text_dim: json
            .get("textDim")
            .and_then(Json::as_str)
            .and_then(hex)
            .unwrap_or(text_muted),
        accent,
        accent_fill: json
            .get("accentFill")
            .and_then(Json::as_str)
            .and_then(hex)
            .unwrap_or(accent),
        selection: json
            .get("selection")
            .and_then(Json::as_str)
            .and_then(hex)
            .unwrap_or(mix(accent, card, 0.35)),
        gutter: colors("gutter", 2)
            .and_then(|list| list.try_into().ok())
            .unwrap_or([mix(text, card, 0.35), text]),
        error: json
            .get("error")
            .and_then(Json::as_str)
            .and_then(hex)
            .unwrap_or(Color::hex(0xE06C75)),
        warning: json
            .get("warning")
            .and_then(Json::as_str)
            .and_then(hex)
            .unwrap_or(Color::hex(0xE5C07B)),
        git: colors("git", 4)?.try_into().ok()?,
        tinted_tab: json
            .get("tintedTab")
            .and_then(Json::as_bool)
            .unwrap_or(true),
        syntax: colors("syntax", 9)?.try_into().ok()?,
        ansi: colors("ansi", 16)
            .and_then(|list| list.try_into().ok())
            .unwrap_or(if dark {
                DEFAULT_ANSI_DARK
            } else {
                DEFAULT_ANSI_LIGHT
            }),
    };
    let name = json
        .get("name")
        .and_then(Json::as_str)
        .unwrap_or(id)
        .to_owned();
    Some((name, Palette::build(tokens)))
}

/// `"#RRGGBB"` 或 `"RRGGBB"` 的颜色。
fn hex(text: &str) -> Option<Color> {
    let digits = text.strip_prefix('#').unwrap_or(text);
    (digits.len() == 6).then(|| u32::from_str_radix(digits, 16).ok().map(Color::hex))?
}

/// 未写 `ansi` 时终端用的默认 16 色（深浅两套，与内置主题同色系的中性配色）。
const DEFAULT_ANSI_DARK: [Color; 16] = [
    Color::hex(0x2A2E36),
    Color::hex(0xE06C75),
    Color::hex(0x98C379),
    Color::hex(0xE5C07B),
    Color::hex(0x61AFEF),
    Color::hex(0xC678DD),
    Color::hex(0x56B6C2),
    Color::hex(0xC9CED6),
    Color::hex(0x3A3F49),
    Color::hex(0xF59A98),
    Color::hex(0xA6D6A0),
    Color::hex(0xEBCD88),
    Color::hex(0x91BCF5),
    Color::hex(0xDAAAE0),
    Color::hex(0x5FC6C2),
    Color::hex(0xE6EAF0),
];

/// 浅色主题的默认终端 16 色。
const DEFAULT_ANSI_LIGHT: [Color; 16] = [
    Color::hex(0x3B4048),
    Color::hex(0xC4442E),
    Color::hex(0x2E8540),
    Color::hex(0x9A6B0F),
    Color::hex(0x2F68A8),
    Color::hex(0x8A44B5),
    Color::hex(0x16806F),
    Color::hex(0x5F6570),
    Color::hex(0x6B7280),
    Color::hex(0xD9475D),
    Color::hex(0x3E9C58),
    Color::hex(0xB98618),
    Color::hex(0x2E86C7),
    Color::hex(0x8454D4),
    Color::hex(0x1F948C),
    Color::hex(0x4767DB),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// 完整的主题文件：必填键齐全、可选键缺省推算。
    #[test]
    fn parses_required_colors_and_fills_defaults() {
        let (name, palette) = parse(
            "my-theme",
            r##"{
                // 行首注释与尾逗号都可以有（JSONC）。
                "name": "我的主题", "dark": true,
                "frame": "#15171C", "card": "#1B1E24", "text": "#D4D8DF",
                "accent": "#78AEF2", "accentFill": "#3D6FC4", "selection": "#2C4468",
                "gutter": ["#4E5562", "#A9B0BC"],
                "error": "#F07F7F", "warning": "#E2BA6A",
                "git": ["#72A7EC", "#5FBFA0", "#EB7F6C", "#D898E0"],
                "syntax": ["#D4D8DF", "#D7915C", "#6DDAD6", "#75B18B", "#C28EC4",
                           "#7F8796", "#6FB7F0", "#E4CF7A", "#F9AEB9"],
            }"##,
        )
        .expect("完整配置可解析");
        assert_eq!(name, "我的主题");
        assert!(palette.dark);
        assert_eq!(palette.text, Color::hex(0xD4D8DF));
        // 缺省的 ansi 用深色默认终端色；括号色从关键字色派生。
        assert_eq!(palette.ansi[0], Color::hex(0x2A2E36));
        assert_eq!(
            palette.brackets[0],
            Color::hex(0xD7915C),
            "括号色从关键字色派生"
        );
    }

    /// 缺少必填键时解析失败。
    #[test]
    fn missing_required_colors_fail() {
        let partial = r##"{ "dark": true, "frame": "#15171C" }"##;
        assert!(parse("x", partial).is_none());
        // `dark` 缺失同样失败。
        let no_dark = r##"{ "frame": "#15171C", "card": "#1B1E24", "text": "#D4D8DF",
            "accent": "#78AEF2", "git": ["#111111", "#222222", "#333333", "#444444"],
            "syntax": ["#111111", "#222222", "#333333", "#444444", "#555555",
                       "#666666", "#777777", "#888888", "#999999"] }"##;
        assert!(parse("x", no_dark).is_none());
    }

    /// 颜色的写法：带与不带 `#` 都认，长度不对或非十六进制时拒绝。
    #[test]
    fn hex_takes_six_digits_with_or_without_a_hash() {
        assert_eq!(hex("#78AEF2"), Some(Color::hex(0x78AEF2)));
        assert_eq!(hex("78AEF2"), Some(Color::hex(0x78AEF2)));
        assert_eq!(hex("#78AE"), None);
        assert_eq!(hex("#7XAEF2"), None);
    }
}
