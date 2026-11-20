//! A small Discord-flavored markdown parser.
//!
//! The AST is intentionally UI-free. The GPUI shell walks it when painting a
//! message. Supported in Milestone 1: paragraphs, bold, italic, inline code,
//! fenced code, links, user mentions, spoilers, quotes, and headings.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading {
        level: u8,
        inlines: Vec<Inline>,
    },
    Quote {
        inlines: Vec<Inline>,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    Paragraph {
        inlines: Vec<Inline>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Bold(Vec<Inline>),
    Italic(Vec<Inline>),
    Code(String),
    Link { text: String, href: String },
    Mention { id: String },
    Spoiler(Vec<Inline>),
}

pub fn parse(input: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut lines = input.split('\n').peekable();
    let mut paragraph: Vec<&str> = Vec::new();

    let flush_paragraph = |paragraph: &mut Vec<&str>, blocks: &mut Vec<Block>| {
        if paragraph.is_empty() {
            return;
        }
        let text = paragraph.join("\n");
        paragraph.clear();
        if !text.trim().is_empty() {
            blocks.push(Block::Paragraph {
                inlines: parse_inlines(&text),
            });
        }
    };

    while let Some(line) = lines.next() {
        if line.starts_with("```") {
            flush_paragraph(&mut paragraph, &mut blocks);
            let language = {
                let lang = line.trim_start_matches('`').trim();
                if lang.is_empty() {
                    None
                } else {
                    Some(lang.to_owned())
                }
            };
            let mut body = Vec::new();
            for inner in lines.by_ref() {
                if inner.starts_with("```") {
                    break;
                }
                body.push(inner);
            }
            blocks.push(Block::Code {
                language,
                text: body.join("\n"),
            });
            continue;
        }
        if line.starts_with("# ") || line.starts_with("## ") || line.starts_with("### ") {
            flush_paragraph(&mut paragraph, &mut blocks);
            let level = line.chars().take_while(|c| *c == '#').count() as u8;
            let text = line.trim_start_matches('#').trim();
            blocks.push(Block::Heading {
                level,
                inlines: parse_inlines(text),
            });
            continue;
        }
        if let Some(rest) = line.strip_prefix("> ") {
            flush_paragraph(&mut paragraph, &mut blocks);
            blocks.push(Block::Quote {
                inlines: parse_inlines(rest),
            });
            continue;
        }
        if line.trim().is_empty() {
            flush_paragraph(&mut paragraph, &mut blocks);
            continue;
        }
        paragraph.push(line);
    }
    flush_paragraph(&mut paragraph, &mut blocks);
    blocks
}

fn parse_inlines(input: &str) -> Vec<Inline> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut index = 0;
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut Vec<Inline>| {
        if !text.is_empty() {
            out.push(Inline::Text(std::mem::take(text)));
        }
    };
    while index < chars.len() {
        if chars[index] == '`' {
            flush(&mut text, &mut out);
            index += 1;
            let start = index;
            while index < chars.len() && chars[index] != '`' {
                index += 1;
            }
            let code: String = chars[start..index].iter().collect();
            if index < chars.len() {
                index += 1;
            }
            out.push(Inline::Code(code));
            continue;
        }
        if starts_with(&chars, index, "**") || starts_with(&chars, index, "__") {
            let marker = [chars[index], chars[index + 1]];
            if let Some(end) = find_marker(&chars, index + 2, &marker) {
                flush(&mut text, &mut out);
                let inner: String = chars[index + 2..end].iter().collect();
                out.push(Inline::Bold(parse_inlines(&inner)));
                index = end + 2;
                continue;
            }
        }
        if chars[index] == '*' || chars[index] == '_' {
            let marker = chars[index];
            if let Some(end) = find_char(&chars, index + 1, marker) {
                flush(&mut text, &mut out);
                let inner: String = chars[index + 1..end].iter().collect();
                out.push(Inline::Italic(parse_inlines(&inner)));
                index = end + 1;
                continue;
            }
        }
        if starts_with(&chars, index, "||") {
            if let Some(end) = find_marker(&chars, index + 2, &['|', '|']) {
                flush(&mut text, &mut out);
                let inner: String = chars[index + 2..end].iter().collect();
                out.push(Inline::Spoiler(parse_inlines(&inner)));
                index = end + 2;
                continue;
            }
        }
        if starts_with(&chars, index, "<@") {
            if let Some(end) = find_char(&chars, index, '>') {
                let raw: String = chars[index + 2..end].iter().collect();
                let id = raw.trim_start_matches('!').to_owned();
                if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
                    flush(&mut text, &mut out);
                    out.push(Inline::Mention { id });
                    index = end + 1;
                    continue;
                }
            }
        }
        if chars[index] == '[' {
            if let Some((text_end, href_end)) = find_link(&chars, index) {
                flush(&mut text, &mut out);
                let label: String = chars[index + 1..text_end].iter().collect();
                let href: String = chars[text_end + 2..href_end].iter().collect();
                out.push(Inline::Link { text: label, href });
                index = href_end + 1;
                continue;
            }
        }
        text.push(chars[index]);
        index += 1;
    }
    flush(&mut text, &mut out);
    out
}

fn starts_with(chars: &[char], index: usize, marker: &str) -> bool {
    let marker: Vec<char> = marker.chars().collect();
    chars[index..].starts_with(&marker)
}

fn find_marker(chars: &[char], from: usize, marker: &[char]) -> Option<usize> {
    chars[from..]
        .windows(marker.len())
        .position(|window| window == marker)
        .map(|pos| from + pos)
}

fn find_char(chars: &[char], from: usize, needle: char) -> Option<usize> {
    chars[from..]
        .iter()
        .position(|c| *c == needle)
        .map(|pos| from + pos)
}

fn find_link(chars: &[char], start: usize) -> Option<(usize, usize)> {
    let close_label = find_char(chars, start + 1, ']')?;
    if close_label + 1 >= chars.len() || chars[close_label + 1] != '(' {
        return None;
    }
    let close_href = find_char(chars, close_label + 2, ')')?;
    Some((close_label, close_href))
}

/// Plain-text rendering used by tests and the voice-status line.
pub fn to_plain(blocks: &[Block]) -> String {
    fn inlines(items: &[Inline], out: &mut String) {
        for item in items {
            match item {
                Inline::Text(text) => out.push_str(text),
                Inline::Bold(inner) | Inline::Italic(inner) | Inline::Spoiler(inner) => {
                    inlines(inner, out);
                }
                Inline::Code(text) => out.push_str(text),
                Inline::Link { text, .. } => out.push_str(text),
                Inline::Mention { id } => {
                    out.push('@');
                    out.push_str(id);
                }
            }
        }
    }
    let mut out = String::new();
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        match block {
            Block::Heading { inlines: items, .. } | Block::Paragraph { inlines: items } => {
                inlines(items, &mut out);
            }
            Block::Quote { inlines: items } => {
                out.push_str("> ");
                inlines(items, &mut out);
            }
            Block::Code { text, .. } => out.push_str(text),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_discord_subset() {
        let source = "# Hello\n\nA **bold** and *italic* note with `code`, <@123>, ||secret|| and [docs](https://example.com).\n\n```rs\nlet x = 1;\n```\n\n> quoted";
        let blocks = parse(source);
        assert!(matches!(blocks[0], Block::Heading { level: 1, .. }));
        let Block::Paragraph { inlines } = &blocks[1] else {
            panic!("expected paragraph");
        };
        assert!(inlines.iter().any(|item| matches!(item, Inline::Bold(_))));
        assert!(inlines.iter().any(|item| matches!(item, Inline::Italic(_))));
        assert!(inlines.iter().any(|item| matches!(item, Inline::Code(_))));
        assert!(inlines
            .iter()
            .any(|item| matches!(item, Inline::Mention { id } if id == "123")));
        assert!(inlines
            .iter()
            .any(|item| matches!(item, Inline::Spoiler(_))));
        assert!(inlines.iter().any(
            |item| matches!(item, Inline::Link { href, .. } if href == "https://example.com")
        ));
        assert!(matches!(blocks[2], Block::Code { .. }));
        assert!(matches!(blocks[3], Block::Quote { .. }));
        let plain = to_plain(&blocks);
        assert!(plain.contains("bold"));
        assert!(plain.contains("@123"));
        assert!(plain.contains("let x = 1;"));
    }
}
