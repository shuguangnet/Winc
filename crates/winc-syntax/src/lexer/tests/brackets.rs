//! 括号对着色深度的词法测试。

use super::*;

/// 括号深度：代码里的成对括号计数，字符串、注释与字符字面量中的不计；多余的右括号按 0 处理。
#[test]
fn bracket_depth_tracks_code_brackets_only() {
    let depth_of = |file: &str, line: &str, state: LineState| {
        let chars: Vec<char> = line.chars().collect();
        let mut depth = 0;
        let mut tokens = Vec::new();
        let next = highlight_line(detect(file), &chars, state, &mut tokens, &mut depth);
        (depth, next)
    };

    // 嵌套与配对：行尾回到 0。
    assert_eq!(depth_of("a.rs", "f(g(1), [2])", LineState::Normal).0, 0);
    // 左括号跨行：下一行从 2 起，再退回。
    assert_eq!(
        depth_of("a.rs", "if a(b(", LineState::Normal),
        (2, LineState::Normal)
    );
    let (depth, _) = depth_of("a.rs", "1)];", LineState::Normal);
    assert_eq!(depth, 0, "多余右括号按 0 处理");
    // 字符串、注释、字符字面量中的括号不计。
    assert_eq!(
        depth_of("a.rs", "let s = \"()(\"; // )", LineState::Normal).0,
        0
    );
    assert_eq!(depth_of("a.py", "x = '(' + f(1)", LineState::Normal).0, 0);
    // 跨行字符串里的括号不计；行首状态不是 Normal 时不计。
    let (depth, next) = depth_of("a.py", "s = \"((", LineState::Normal);
    assert_eq!(depth, 0, "未闭合字符串中的括号不计");
    if next == LineState::DoubleQuote {
        assert_eq!(depth_of("a.py", ")((", next).0, 0);
    }
    // 块注释同样不计。
    let (depth, next) = depth_of("a.rs", "/* ((", LineState::Normal);
    assert!(depth == 0 && next != LineState::Normal);
    assert_eq!(depth_of("a.rs", "(((", next).0, 0);
}

/// 括号深度跨行累加：`Highlighter` 与状态一起逐行推算，编辑后随之失效。
#[test]
fn highlighter_tracks_depth_per_line() {
    use crate::Highlighter;
    let text = simple_text("a(\nb(\nc)\nd)\nlet s = \"(\";\nlet x = [1];\n");
    let mut highlighter = Highlighter::new(detect("a.rs"));
    let depths: Vec<i32> = (0..6)
        .map(|line| {
            highlighter.state_at(&text, line, 100);
            highlighter.known_depth(line).unwrap()
        })
        .collect();
    assert_eq!(
        depths,
        [0, 1, 2, 1, 0, 0],
        "逐行累加，行首是上一行行尾的层数"
    );

    // 从第 2 行编辑：第 2 行本身保留，其后各行失效，重新推算的结果不变。
    highlighter.invalidate_from(2);
    assert_eq!(highlighter.known_depth(2), Some(2));
    assert_eq!(highlighter.known_depth(3), None);
    highlighter.state_at(&text, 5, 100);
    assert_eq!(highlighter.known_depth(5), Some(0));
}

/// `simple_text` 构造逐行文本的辅助（`Lines` 的最小实现）。
fn simple_text(content: &str) -> impl Lines {
    use winc_core::text::Line;

    struct Text(Vec<String>);

    impl Lines for Text {
        fn line_count(&self) -> usize {
            self.0.len()
        }

        fn line_start(&self, line: usize) -> usize {
            self.0[..line.min(self.0.len())]
                .iter()
                .map(|l| l.len() + 1)
                .sum()
        }

        fn line_of(&self, pos: usize) -> usize {
            let mut end = 0;
            for (line, l) in self.0.iter().enumerate() {
                end += l.len() + 1;
                if pos < end {
                    return line;
                }
            }
            self.0.len().saturating_sub(1)
        }

        fn line_content(&self, line: usize) -> Line<'_> {
            Line::Owned(self.0[line.min(self.0.len())].clone())
        }

        fn substring(&self, from: usize, to: usize) -> String {
            let mut out = String::new();
            let (first, last) = (
                self.line_of(from),
                self.line_of(to.saturating_sub(1).max(from)),
            );
            for line in first..=last {
                let (start, content) = (self.line_start(line), &self.0[line]);
                let at = |line: usize| self.line_start(line) + self.0[line].len() + 1;
                let (a, b) = (
                    from.saturating_sub(start).min(content.len()),
                    if line == last {
                        to.saturating_sub(start).min(content.len())
                    } else {
                        content.len()
                    },
                );
                let _ = at(line);
                if line == first {
                    out.push_str(&content[a.min(content.len())..]);
                } else if line == last {
                    out.push_str(&content[..b]);
                } else {
                    out.push_str(content);
                }
                if line != last {
                    out.push('\n');
                }
            }
            out
        }

        fn end_pos(&self) -> usize {
            self.line_start(self.0.len())
        }
    }

    Text(content.lines().map(str::to_owned).collect())
}
