//! VS Code 系列主题：Dark Modern、Dark+（经典蓝底状态栏）与 Light Modern。
//!
//! 表面层级严格对齐 Visual Studio Code：编辑区与侧栏/活动栏/标题栏/标签栏/状态栏通过明度拉开，
//! 搭配 1px 细边线与强调色指示条；所有语法色与终端 16 色均满足对比度约束。

use super::*;

const fn hex(rgb: u32) -> Color {
    Color::hex(rgb)
}

/// 「VS Code Dark Modern」：默认深色主题。
///
/// `#1F1F1F` 编辑区、`#181818` 侧栏/活动栏/标题栏/标签栏/状态栏、`#2B2B2B` 边线、`#0078D4` 强调色。
pub static VSCODE_DARK_MODERN: Palette = Palette::build(Tokens {
    dark: true,
    frame: hex(0x181818),
    card: hex(0x1F1F1F),
    popup: hex(0x252526),
    text: hex(0xCCCCCC),
    text_muted: hex(0x9D9D9D),
    text_dim: hex(0x6E7681),
    accent: hex(0x3794FF),
    accent_fill: hex(0x0078D4),
    selection: hex(0x264F78),
    gutter: [hex(0x6E7681), hex(0xCCCCCC)],
    error: hex(0xF14C4C),
    warning: hex(0xCCA700),
    git: [hex(0xE2C08D), hex(0x73C991), hex(0xC74E39), hex(0xE4676B)],
    tinted_tab: false,
    syntax: [
        hex(0xCCCCCC),
        hex(0x569CD6),
        hex(0x4EC9B0),
        hex(0xCE9178),
        hex(0xB5CEA8),
        hex(0x6A9955),
        hex(0xDCDCAA),
        hex(0xC586C0),
        hex(0x9CDCFE),
    ],
    ansi: [
        hex(0x181818),
        hex(0xCD3131),
        hex(0x0DBC79),
        hex(0xE5E510),
        hex(0x2472C8),
        hex(0xBC3FBC),
        hex(0x11A8CD),
        hex(0xE5E5E5),
        hex(0x727272),
        hex(0xF14C4C),
        hex(0x23D18B),
        hex(0xF5F543),
        hex(0x3B8EEA),
        hex(0xD670D6),
        hex(0x29B8DB),
        hex(0xFFFFFF),
    ],
});

/// 「VS Code Dark+」：经典深色主题，带 `#333333` 活动栏与 `#007ACC` 亮蓝状态栏。
pub static VSCODE_DARK_PLUS: Palette = Palette::build(Tokens {
    dark: true,
    frame: hex(0x252526),
    card: hex(0x1E1E1E),
    popup: hex(0x252526),
    text: hex(0xD4D4D4),
    text_muted: hex(0x9D9D9D),
    text_dim: hex(0x6E7681),
    accent: hex(0x3794FF),
    accent_fill: hex(0x007ACC),
    selection: hex(0x264F78),
    gutter: [hex(0x858585), hex(0xC6C6C6)],
    error: hex(0xF14C4C),
    warning: hex(0xCCA700),
    git: [hex(0xE2C08D), hex(0x73C991), hex(0xC74E39), hex(0xE4676B)],
    tinted_tab: false,
    syntax: [
        hex(0xD4D4D4),
        hex(0x569CD6),
        hex(0x4EC9B0),
        hex(0xCE9178),
        hex(0xB5CEA8),
        hex(0x6A9955),
        hex(0xDCDCAA),
        hex(0xC586C0),
        hex(0x9CDCFE),
    ],
    ansi: [
        hex(0x000000),
        hex(0xCD3131),
        hex(0x0DBC79),
        hex(0xE5E510),
        hex(0x2472C8),
        hex(0xBC3FBC),
        hex(0x11A8CD),
        hex(0xE5E5E5),
        hex(0x727272),
        hex(0xF14C4C),
        hex(0x23D18B),
        hex(0xF5F543),
        hex(0x3B8EEA),
        hex(0xD670D6),
        hex(0x29B8DB),
        hex(0xFFFFFF),
    ],
})
.with_chrome(
    hex(0x3C3C3C),
    hex(0x333333),
    hex(0x252526),
    hex(0x252526),
    hex(0x007ACC),
    hex(0xFFFFFF),
);

/// 「VS Code Light Modern」：默认浅色主题。
///
/// `#FFFFFF` 编辑区、`#F8F8F8` 侧栏/活动栏/标题栏/标签栏/状态栏、`#E5E5E5` 边线、`#005FB8` 强调色。
pub static VSCODE_LIGHT_MODERN: Palette = Palette::build(Tokens {
    dark: false,
    frame: hex(0xF8F8F8),
    card: hex(0xFFFFFF),
    popup: hex(0xFFFFFF),
    text: hex(0x3B3B3B),
    text_muted: hex(0x616161),
    text_dim: hex(0x8E8E8E),
    accent: hex(0x005FB8),
    accent_fill: hex(0x005FB8),
    selection: hex(0xADD6FF),
    gutter: [hex(0x6E7681), hex(0x171184)],
    error: hex(0xC72E0F),
    warning: hex(0xBF8803),
    git: [hex(0x895503), hex(0x388A34), hex(0xAD0707), hex(0xAD0707)],
    tinted_tab: false,
    syntax: [
        hex(0x1F1F1F),
        hex(0x0000FF),
        hex(0x1F6B82),
        hex(0xA31515),
        hex(0x06724A),
        hex(0x008000),
        hex(0x795E26),
        hex(0xA500CE),
        hex(0x001080),
    ],
    ansi: [
        hex(0x000000),
        hex(0xCD3131),
        hex(0x1A7F1A),
        hex(0x856A00),
        hex(0x0451A5),
        hex(0xBC05BC),
        hex(0x0598BC),
        hex(0x555555),
        hex(0x727272),
        hex(0xCD3131),
        hex(0x14A314),
        hex(0xB08600),
        hex(0x0451A5),
        hex(0xBC05BC),
        hex(0x0598BC),
        hex(0xA5A5A5),
    ],
});
