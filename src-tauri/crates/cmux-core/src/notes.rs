//! A deliberately small markdown subset for release notes.
//!
//! Notes come from the GitHub API, so they are remote text that ends up in
//! the *host* webview — the one holding the IPC bridge. Parsing here, into
//! structured blocks, means the frontend never interprets that text at all:
//! it maps each span to a React element, and React escapes every string.
//! Raw HTML in a note therefore renders as the characters it is made of,
//! and only `http(s)` links are navigable.
//!
//! Supported because it is what release notes actually use: ATX headings,
//! paragraphs, bullet lists, fenced code, thematic breaks, and the inline
//! run of code, bold, italic and links. Anything else degrades to text
//! rather than disappearing.

use cmux_protocol::{NoteBlock, NoteSpan};

/// Parses a release body into blocks.
pub fn parse(src: &str) -> Vec<NoteBlock> {
    let normalized = src.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mut blocks: Vec<NoteBlock> = Vec::new();
    // Paragraph and list lines accumulate until something ends them.
    let mut para: Vec<String> = Vec::new();
    let mut items: Vec<String> = Vec::new();

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];

        // A fence swallows lines verbatim until it closes, so markdown
        // inside a code block is never interpreted. An unterminated fence
        // runs to the end rather than dropping the rest of the notes.
        if let Some(lang) = fence_lang(line) {
            flush_para(&mut para, &mut blocks);
            flush_list(&mut items, &mut blocks);
            let mut body: Vec<&str> = Vec::new();
            i += 1;
            while i < lines.len() && fence_lang(lines[i]).is_none() {
                body.push(lines[i]);
                i += 1;
            }
            i += 1; // the closing fence, or past the end
            blocks.push(NoteBlock::Code {
                text: body.join("\n"),
                lang,
            });
            continue;
        }

        if line.trim().is_empty() {
            flush_para(&mut para, &mut blocks);
            flush_list(&mut items, &mut blocks);
            i += 1;
            continue;
        }

        if is_rule(line) {
            flush_para(&mut para, &mut blocks);
            flush_list(&mut items, &mut blocks);
            blocks.push(NoteBlock::Rule);
            i += 1;
            continue;
        }

        if let Some((level, text)) = heading(line) {
            flush_para(&mut para, &mut blocks);
            flush_list(&mut items, &mut blocks);
            blocks.push(NoteBlock::Heading {
                level,
                spans: parse_inline(text),
            });
            i += 1;
            continue;
        }

        if let Some(text) = bullet(line) {
            flush_para(&mut para, &mut blocks);
            items.push(text.to_string());
            i += 1;
            continue;
        }

        // A plain line inside a list continues its last item (release notes
        // wrap long bullets); otherwise it continues the paragraph.
        match items.last_mut() {
            Some(last) => {
                last.push(' ');
                last.push_str(line.trim());
            }
            None => para.push(line.trim().to_string()),
        }
        i += 1;
    }

    flush_para(&mut para, &mut blocks);
    flush_list(&mut items, &mut blocks);
    blocks
}

fn flush_para(para: &mut Vec<String>, blocks: &mut Vec<NoteBlock>) {
    if !para.is_empty() {
        blocks.push(NoteBlock::Paragraph {
            spans: parse_inline(&para.join(" ")),
        });
        para.clear();
    }
}

fn flush_list(items: &mut Vec<String>, blocks: &mut Vec<NoteBlock>) {
    if !items.is_empty() {
        blocks.push(NoteBlock::List {
            items: items.iter().map(|i| parse_inline(i)).collect(),
        });
        items.clear();
    }
}

/// ```` ```rust ```` → `Some(Some("rust"))`, ```` ``` ```` → `Some(None)`.
fn fence_lang(line: &str) -> Option<Option<String>> {
    let rest = line.trim_end().strip_prefix("```")?;
    if rest.is_empty() {
        return Some(None);
    }
    // A fence's info string is a bare word; anything else is not a fence we
    // claim to understand.
    rest.chars()
        .all(|c| c.is_ascii_alphanumeric())
        .then(|| Some(rest.to_string()))
}

fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3
        && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if !(1..=3).contains(&hashes) {
        return None;
    }
    let rest = &line[hashes..];
    let text = rest.strip_prefix(' ')?;
    Some((hashes as u8, text.trim()))
}

fn bullet(line: &str) -> Option<&str> {
    let t = line.trim_start();
    for marker in ['-', '*', '+'] {
        if let Some(rest) = t.strip_prefix(marker) {
            if let Some(text) = rest.strip_prefix(' ') {
                return Some(text.trim());
            }
        }
    }
    None
}

/// Splits text into inline spans. Scans left to right, taking the earliest
/// marker that closes; an unclosed marker is literal text.
pub fn parse_inline(src: &str) -> Vec<NoteSpan> {
    let mut spans: Vec<NoteSpan> = Vec::new();
    let mut text = String::new();
    let bytes = src.as_bytes();
    let mut i = 0;

    while i < src.len() {
        // Only ASCII markers are matched, so byte indexing is safe here;
        // everything else is pushed through as whole chars below.
        let matched = match bytes[i] {
            b'`' => take_delimited(src, i, "`", "`").map(|(inner, end)| {
                (
                    NoteSpan::Code {
                        text: inner.to_string(),
                    },
                    end,
                )
            }),
            b'[' => take_link(src, i),
            b'*' if src[i..].starts_with("**") => take_delimited(src, i, "**", "**")
                .map(|(inner, end)| (NoteSpan::Strong { text: inner.to_string() }, end)),
            b'_' if src[i..].starts_with("__") => take_delimited(src, i, "__", "__")
                .map(|(inner, end)| (NoteSpan::Strong { text: inner.to_string() }, end)),
            b'*' => take_delimited(src, i, "*", "*")
                .map(|(inner, end)| (NoteSpan::Em { text: inner.to_string() }, end)),
            b'_' => take_delimited(src, i, "_", "_")
                .map(|(inner, end)| (NoteSpan::Em { text: inner.to_string() }, end)),
            _ => None,
        };

        match matched {
            Some((span, end)) => {
                if !text.is_empty() {
                    spans.push(NoteSpan::Text {
                        text: std::mem::take(&mut text),
                    });
                }
                spans.push(span);
                i = end;
            }
            None => {
                // Advance by a whole char so multi-byte text survives.
                let ch = src[i..].chars().next().expect("in bounds");
                text.push(ch);
                i += ch.len_utf8();
            }
        }
    }

    if !text.is_empty() {
        spans.push(NoteSpan::Text { text });
    }
    spans
}

/// The content between `open` at `start` and the next `close`, plus the
/// index just past it. Empty content does not count as a match, so `**`
/// and `____` stay literal.
fn take_delimited<'a>(
    src: &'a str,
    start: usize,
    open: &str,
    close: &str,
) -> Option<(&'a str, usize)> {
    let after_open = start + open.len();
    let rest = src.get(after_open..)?;
    let end = rest.find(close)?;
    if end == 0 {
        return None;
    }
    Some((&rest[..end], after_open + end + close.len()))
}

/// `[text](href)` — and only when the href is one we would open. A
/// `javascript:` or relative href keeps the whole token as literal text, so
/// the note still reads correctly and nothing unnavigable looks clickable.
fn take_link(src: &str, start: usize) -> Option<(NoteSpan, usize)> {
    let rest = &src[start..];
    let close_bracket = rest.find("](")?;
    let text = &rest[1..close_bracket];
    let after = &rest[close_bracket + 2..];
    let close_paren = after.find(')')?;
    let href = &after[..close_paren];
    if href.is_empty() || href.contains(char::is_whitespace) {
        return None;
    }
    let lower = href.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return None;
    }
    let end = start + close_bracket + 2 + close_paren + 1;
    Some((
        NoteSpan::Link {
            text: if text.is_empty() { href.to_string() } else { text.to_string() },
            href: href.to_string(),
        },
        end,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> NoteSpan {
        NoteSpan::Text { text: s.into() }
    }

    #[test]
    fn parses_a_real_release_body() {
        let body = "\
## Fixed: a black stripe

A thin black band sat under the last line.

`FitAddon` rounds rows down, so a terminal is **slightly** shorter.

## Install

See the [README](https://github.com/SuperHomer/mirador#install-macos).
";
        let blocks = parse(body);
        assert_eq!(blocks.len(), 5);
        assert!(matches!(&blocks[0], NoteBlock::Heading { level: 2, .. }));
        assert!(matches!(&blocks[1], NoteBlock::Paragraph { .. }));
        // Inline code and bold survive inside a paragraph.
        let NoteBlock::Paragraph { spans } = &blocks[2] else {
            panic!("expected a paragraph");
        };
        assert_eq!(spans[0], NoteSpan::Code { text: "FitAddon".into() });
        assert!(spans.contains(&NoteSpan::Strong { text: "slightly".into() }));
        let NoteBlock::Paragraph { spans } = &blocks[4] else {
            panic!("expected a paragraph");
        };
        assert!(spans.iter().any(|s| matches!(s, NoteSpan::Link { text, .. } if text == "README")));
    }

    #[test]
    fn html_and_unopenable_links_stay_text() {
        // The whole point of parsing here: nothing becomes markup.
        assert_eq!(
            parse_inline("<img src=x onerror=\"alert(1)\">"),
            vec![text("<img src=x onerror=\"alert(1)\">")]
        );
        // A javascript: URL must never become a link.
        let spans = parse_inline("[click](javascript:alert(1))");
        assert!(spans.iter().all(|s| matches!(s, NoteSpan::Text { .. })));
        assert!(!spans.iter().any(|s| matches!(s, NoteSpan::Link { .. })));
        // Nor a relative one, which we have no base to resolve.
        let spans = parse_inline("[rel](/relative/path)");
        assert!(spans.iter().all(|s| matches!(s, NoteSpan::Text { .. })));
        // data: is the third shape of the same trick.
        let spans = parse_inline("[x](data:text/html,<script>alert(1)</script>)");
        assert!(!spans.iter().any(|s| matches!(s, NoteSpan::Link { .. })));
        // An ordinary https link does become one.
        assert_eq!(
            parse_inline("[ok](https://example.com)"),
            vec![NoteSpan::Link {
                text: "ok".into(),
                href: "https://example.com".into()
            }]
        );
        // Case is not a way past the check.
        assert!(matches!(
            parse_inline("[ok](HTTPS://example.com)").as_slice(),
            [NoteSpan::Link { .. }]
        ));
    }

    #[test]
    fn inline_runs_in_order() {
        assert_eq!(
            parse_inline("a **b** c `d` e [f](https://g.h) i"),
            vec![
                text("a "),
                NoteSpan::Strong { text: "b".into() },
                text(" c "),
                NoteSpan::Code { text: "d".into() },
                text(" e "),
                NoteSpan::Link {
                    text: "f".into(),
                    href: "https://g.h".into()
                },
                text(" i"),
            ]
        );
        // `**` must win over `*`, or bold reads as two empty italics.
        assert_eq!(
            parse_inline("**bold** and *em* and __b__ and _e_"),
            vec![
                NoteSpan::Strong { text: "bold".into() },
                text(" and "),
                NoteSpan::Em { text: "em".into() },
                text(" and "),
                NoteSpan::Strong { text: "b".into() },
                text(" and "),
                NoteSpan::Em { text: "e".into() },
            ]
        );
    }

    #[test]
    fn unclosed_markers_are_literal() {
        // Two `*` on one line do pair up — arithmetic in prose is the
        // known cost of supporting `*em*` at all.
        assert_eq!(
            parse_inline("2 * 3 * 4"),
            vec![text("2 "), NoteSpan::Em { text: " 3 ".into() }, text(" 4")]
        );
        // A lone marker with nothing to close against stays text.
        assert_eq!(parse_inline("a * b"), vec![text("a * b")]);
        assert_eq!(parse_inline("`unclosed"), vec![text("`unclosed")]);
        assert_eq!(parse_inline("****"), vec![text("****")]);
    }

    #[test]
    fn lists_wrap_and_fences_are_verbatim() {
        let blocks = parse("- first item\n  continued here\n- second\n");
        let NoteBlock::List { items } = &blocks[0] else {
            panic!("expected a list, got {blocks:?}");
        };
        assert_eq!(items.len(), 2);
        assert_eq!(items[0], vec![text("first item continued here")]);

        // Markdown inside a fence is not interpreted.
        let blocks = parse("```sh\n# not a heading\n**not bold**\n```\n");
        assert_eq!(
            blocks,
            vec![NoteBlock::Code {
                text: "# not a heading\n**not bold**".into(),
                lang: Some("sh".into()),
            }]
        );
        // An unterminated fence keeps what follows rather than dropping it.
        let blocks = parse("para\n\n```\nstill here");
        assert_eq!(blocks.len(), 2);
        assert!(matches!(&blocks[1], NoteBlock::Code { text, .. } if text == "still here"));
    }

    #[test]
    fn headings_rules_and_non_headings() {
        assert!(matches!(
            parse("### three").as_slice(),
            [NoteBlock::Heading { level: 3, .. }]
        ));
        // Four hashes is not a heading level we render, and `#tag` is not a
        // heading at all.
        assert!(matches!(parse("#### four").as_slice(), [NoteBlock::Paragraph { .. }]));
        assert!(matches!(parse("#notaheading").as_slice(), [NoteBlock::Paragraph { .. }]));
        assert_eq!(parse("---\n"), vec![NoteBlock::Rule]);
        assert_eq!(parse("***\n"), vec![NoteBlock::Rule]);
    }

    #[test]
    fn multibyte_text_is_not_split() {
        // Byte indexing around markers must not cut a char in half.
        let spans = parse_inline("une **mise à jour** — déjà là");
        assert_eq!(spans[0], text("une "));
        assert_eq!(spans[1], NoteSpan::Strong { text: "mise à jour".into() });
        assert_eq!(spans[2], text(" — déjà là"));
    }

    #[test]
    fn empty_input_is_no_blocks() {
        assert!(parse("").is_empty());
        assert!(parse("\n\n\n").is_empty());
    }
}
