//! 将提交说明中的常见 `:gitmoji:` 短代码替换为对应 Emoji（对齐 `gitHistoryVSCode` 的 `gitmojify.ts`）。

use std::borrow::Cow;

/// 将 `text` 中的 `:shortcode:` 替换为对应的 Unicode Emoji；不含 `:` 时零分配直接借用。
pub(super) fn gitmojify(text: &str) -> Cow<'_, str> {
    if !text.contains(':') {
        return Cow::Borrowed(text);
    }
    let bytes = text.as_bytes();
    let mut out: Option<String> = None;
    let mut last = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b':'
            && let Some(rel_end) = bytes[i + 1..].iter().position(|&b| b == b':')
        {
            let end = i + 1 + rel_end;
            let code = &text[i + 1..end];
            if let Some(emoji) = lookup(code) {
                let buf = out.get_or_insert_with(|| String::with_capacity(text.len()));
                buf.push_str(&text[last..i]);
                buf.push_str(emoji);
                i = end + 1;
                last = i;
                continue;
            }
        }
        i += 1;
    }
    match out {
        Some(mut buf) => {
            buf.push_str(&text[last..]);
            Cow::Owned(buf)
        }
        None => Cow::Borrowed(text),
    }
}

/// 常见 Gitmoji / GitHub Emoji 短代码表。
fn lookup(code: &str) -> Option<&'static str> {
    Some(match code {
        "art" => "🎨",
        "zap" => "⚡",
        "fire" => "🔥",
        "bug" => "🐛",
        "ambulance" => "🚑",
        "sparkles" => "✨",
        "memo" => "📝",
        "rocket" => "🚀",
        "lipstick" => "💄",
        "tada" => "🎉",
        "white_check_mark" => "✅",
        "lock" => "🔒",
        "closed_lock_with_key" => "🔐",
        "bookmark" => "🔖",
        "rotating_light" => "🚨",
        "construction" => "🚧",
        "green_heart" => "💚",
        "arrow_down" => "⬇️",
        "arrow_up" => "⬆️",
        "pushpin" => "📌",
        "construction_worker" => "👷",
        "chart_with_upwards_trend" => "📈",
        "recycle" => "♻️",
        "heavy_plus_sign" => "➕",
        "heavy_minus_sign" => "➖",
        "wrench" => "🔧",
        "hammer" => "🔨",
        "globe_with_meridians" => "🌐",
        "pencil2" => "✏️",
        "poop" => "💩",
        "rewind" => "⏪",
        "twisted_rightwards_arrows" => "🔀",
        "package" => "📦",
        "alien" => "👽",
        "truck" => "🚚",
        "page_facing_up" => "📄",
        "boom" => "💥",
        "bento" => "🍱",
        "wheelchair" => "♿",
        "bulb" => "💡",
        "beers" => "🍻",
        "speech_balloon" => "💬",
        "card_file_box" => "🗃️",
        "loud_sound" => "🔊",
        "mute" => "🔇",
        "busts_in_silhouette" => "👥",
        "children_crossing" => "🚸",
        "building_construction" => "🏗️",
        "iphone" => "📱",
        "clown_face" => "🤡",
        "egg" => "🥚",
        "see_no_evil" => "🙈",
        "camera_flash" => "📸",
        "alembic" => "⚗️",
        "mag" => "🔍",
        "label" => "🏷️",
        "seedling" => "🌱",
        "triangular_flag_on_post" => "🚩",
        "goal_net" => "🥅",
        "dizzy" => "💫",
        "wastebasket" => "🗑️",
        "passport_control" => "🛂",
        "adhesive_bandage" => "🩹",
        "monocle_face" => "🧐",
        "coffin" => "⚰️",
        "test_tube" => "🧪",
        "necktie" => "👔",
        "stethoscope" => "🩺",
        "bricks" => "🧱",
        "technologist" => "🧑‍💻",
        "money_with_wings" => "💸",
        "thread" => "🧵",
        "safety_vest" => "🦺",
        _ => return None,
    })
}
