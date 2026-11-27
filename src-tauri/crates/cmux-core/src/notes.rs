//! A deliberately small markdown subset, for release notes and for the
//! local markdown files a markdown pane shows (READMEs, plans, notes).
//!
//! Notes come from the GitHub API, so they are remote text that ends up in
//! the *host* webview — the one holding the IPC bridge; a file on disk may
//! have been written by anyone, an agent included, and is no safer. Parsing here, into
//! structured blocks, means the frontend never interprets that text at all:
//! it maps each span to a React element, and React escapes every string.
//! Raw HTML in a note therefore renders as the characters it is made of,
//! and only `http(s)` links are navigable.
//!
//! Supported because it is what release notes and READMEs actually use:
//! ATX headings, paragraphs, bullet and numbered lists, blockquotes, fenced
//! code, thematic breaks, and the inline run of code, bold, italic and
//! links. Anything else degrades to text rather than disappearing.

use std::io::Read;
use std::path::{Path, PathBuf};

use cmux_protocol::{MarkdownFiles, NoteBlock, NoteSpan};

/// Larger than any README or plan; a file past it is almost certainly not
/// one, and parsing it would stall the pane for nothing.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Reads and parses a markdown file. The path must be absolute: a relative
/// one would resolve against whatever the app's working directory is.
pub fn read_file(path: &Path) -> Result<Vec<NoteBlock>, String> {
    if !path.is_absolute() {
        return Err(format!("not an absolute path: {}", path.display()));
    }
    // Read through the cap rather than trusting the file's reported size:
    // a FIFO or `/dev/zero` reports 0 and would never stop.
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(format!(
            "{} is larger than the {} KB a markdown pane shows",
            path.display(),
            MAX_FILE_BYTES / 1024
        ));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("{} is not UTF-8 text", path.display()))?;
    Ok(parse(&text))
}

/// More than a palette can usefully list; a tree past it is not one
/// whose markdown anyone picks by name.
pub const MAX_LISTED_FILES: usize = 2000;

/// The markdown files a pane in `dir` can open, as absolute paths, the
/// shallowest first (`README.md` before `docs/plan.md`).
///
/// In a git repository, the whole repository's — tracked and untracked
/// alike, but never what `.gitignore` leaves out, which is what keeps
/// `node_modules` and `target` from burying the rest. Outside one, only
/// `dir`'s own files: a walk down from `~` would never end.
pub fn list_files(dir: &Path) -> Result<MarkdownFiles, String> {
    let (root, mut files): (PathBuf, Vec<PathBuf>) = match crate::git::find_repo_root(&dir.to_string_lossy()) {
        Some(root) => {
            let output = crate::proc::command("git")
                .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
                .current_dir(&root)
                .output()
                .map_err(|e| format!("could not run git: {e}"))?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
            }
            let files = String::from_utf8_lossy(&output.stdout)
                .split('\0')
                .filter(|f| is_markdown(Path::new(f)))
                .map(|f| root.join(f))
                .collect();
            (root, files)
        }
        None => {
            let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let files = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| is_markdown(p) && p.is_file())
                .collect();
            (dir.to_path_buf(), files)
        }
    };
    files.sort_by(|a, b| {
        (a.components().count(), a).cmp(&(b.components().count(), b))
    });
    files.dedup();
    files.truncate(MAX_LISTED_FILES);
    Ok(MarkdownFiles {
        root: root.to_string_lossy().into_owned(),
        files: files.iter().map(|f| f.to_string_lossy().into_owned()).collect(),
    })
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
}

/// Quotes nested deeper than this read as text. Each level is a call of
/// its own, so a line of thousands of `>` would otherwise overflow the stack.
const MAX_QUOTE_DEPTH: usize = 8;

/// Parses a release body or a markdown file into blocks.
pub fn parse(src: &str) -> Vec<NoteBlock> {
    parse_at(src, 0)
}

fn parse_at(src: &str, depth: usize) -> Vec<NoteBlock> {
    let normalized = src.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mut blocks: Vec<NoteBlock> = Vec::new();
    // Paragraph and list lines accumulate until something ends them.
    let mut para: Vec<String> = Vec::new();
    let mut items: Vec<String> = Vec::new();
    let mut ordered = false;

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];

        // A fence swallows lines verbatim until it closes, so markdown
        // inside a code block is never interpreted. An unterminated fence
        // runs to the end rather than dropping the rest of the notes.
        if let Some(lang) = fence_lang(line) {
            flush_para(&mut para, &mut blocks);
            flush_list(&mut items, ordered, &mut blocks);
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
            flush_list(&mut items, ordered, &mut blocks);
            i += 1;
            continue;
        }

        if is_rule(line) {
            flush_para(&mut para, &mut blocks);
            flush_list(&mut items, ordered, &mut blocks);
            blocks.push(NoteBlock::Rule);
            i += 1;
            continue;
        }

        // A run of `>` lines is a quote, and what it quotes is markdown of
        // its own: a list or a fence inside one reads as it would outside.
        if depth < MAX_QUOTE_DEPTH && quoted(line).is_some() {
            flush_para(&mut para, &mut blocks);
            flush_list(&mut items, ordered, &mut blocks);
            let mut inner: Vec<&str> = Vec::new();
            while let Some(rest) = lines.get(i).and_then(|l| quoted(l)) {
                inner.push(rest);
                i += 1;
            }
            blocks.push(NoteBlock::Quote {
                blocks: parse_at(&inner.join("\n"), depth + 1),
            });
            continue;
        }

        // An image on a line of its own becomes a block. Release notes put
        // screenshots between paragraphs, never mid-sentence, and a block
        // is the only place one can be shown at a useful size.
        if let Some((alt, url)) = lone_image(line) {
            flush_para(&mut para, &mut blocks);
            flush_list(&mut items, ordered, &mut blocks);
            blocks.push(NoteBlock::Image { alt, url });
            i += 1;
            continue;
        }
        // One we cannot fetch — a local file beside a README, mostly —
        // stands as its alt text rather than as the raw `![…](…)`.
        if let Some(alt) = unopenable_image_alt(line) {
            flush_para(&mut para, &mut blocks);
            flush_list(&mut items, ordered, &mut blocks);
            if !alt.is_empty() {
                blocks.push(NoteBlock::Paragraph {
                    spans: parse_inline(alt),
                });
            }
            i += 1;
            continue;
        }

        if let Some((level, text)) = heading(line) {
            flush_para(&mut para, &mut blocks);
            flush_list(&mut items, ordered, &mut blocks);
            blocks.push(NoteBlock::Heading {
                level,
                spans: parse_inline(text),
            });
            i += 1;
            continue;
        }

        if let Some((numbered, text)) = list_item(line) {
            flush_para(&mut para, &mut blocks);
            // A switch between `-` and `1.` starts a new list.
            if numbered != ordered {
                flush_list(&mut items, ordered, &mut blocks);
                ordered = numbered;
            }
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
    flush_list(&mut items, ordered, &mut blocks);
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

fn flush_list(items: &mut Vec<String>, ordered: bool, blocks: &mut Vec<NoteBlock>) {
    if !items.is_empty() {
        blocks.push(NoteBlock::List {
            ordered,
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

/// `![alt](url)` alone on a line → its alt text and url. Anything else on
/// the line means it is prose that happens to contain an image, which is
/// handled inline instead.
fn lone_image(line: &str) -> Option<(String, String)> {
    let t = line.trim();
    let rest = t.strip_prefix('!')?;
    let (span, end) = take_link(rest, 0)?;
    // The link must be the whole of what followed the `!`.
    if end != rest.len() {
        return None;
    }
    match span {
        NoteSpan::Link { text, href } => Some((text, href)),
        _ => None,
    }
}

/// `![alt](url)` alone on a line whatever its url → the alt text. Only
/// asked after `lone_image` has turned it down, so the url is one that
/// will not be fetched.
fn unopenable_image_alt(line: &str) -> Option<&str> {
    let rest = line.trim().strip_prefix("![")?.strip_suffix(')')?;
    let (alt, url) = rest.split_once("](")?;
    let plain = !url.is_empty() && !url.contains(|c: char| c.is_whitespace() || c == ')');
    plain.then_some(alt.trim())
}

/// `> text` → `text`; a bare `>` is an empty quoted line.
fn quoted(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('>')?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
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

/// A list item's text, and whether it is numbered: `- a` → `(false, "a")`,
/// `2. b` or `2) b` → `(true, "b")`. The numbers themselves are not kept;
/// the list counts from one, as almost every list in a file does.
fn list_item(line: &str) -> Option<(bool, &str)> {
    let t = line.trim_start();
    for marker in ['-', '*', '+'] {
        if let Some(rest) = t.strip_prefix(marker) {
            if let Some(text) = rest.strip_prefix(' ') {
                return Some((false, text.trim()));
            }
        }
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if !(1..=9).contains(&digits) {
        return None;
    }
    let rest = t[digits..].strip_prefix(['.', ')'])?;
    Some((true, rest.strip_prefix(' ')?.trim()))
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
            // `![alt](url)` inside a sentence: the image marker is dropped
            // and the link kept, rather than leaving a stray `!` behind.
            b'!' if src.as_bytes().get(i + 1) == Some(&b'[') => {
                take_link(src, i + 1)
            }
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
        let NoteBlock::List { items, ordered: false } = &blocks[0] else {
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
    fn an_image_on_its_own_line_is_a_block() {
        let blocks = parse("Before.\n\n![a demo](https://example.com/demo.gif)\n\nAfter.");
        assert_eq!(blocks.len(), 3);
        assert_eq!(
            blocks[1],
            NoteBlock::Image {
                alt: "a demo".into(),
                url: "https://example.com/demo.gif".into()
            }
        );
    }

    #[test]
    fn an_image_in_a_sentence_keeps_the_prose() {
        // Not a block: the line is prose. The `!` must not survive as text.
        let spans = parse_inline("see ![a demo](https://example.com/d.gif) here");
        assert_eq!(
            spans,
            vec![
                text("see "),
                NoteSpan::Link {
                    text: "a demo".into(),
                    href: "https://example.com/d.gif".into()
                },
                text(" here"),
            ]
        );
        // A bare `!` is still a `!`.
        assert_eq!(parse_inline("wow! really"), vec![text("wow! really")]);
    }

    #[test]
    fn an_image_with_an_unopenable_url_is_not_a_block() {
        // The same rule as links: only http(s) is addressable, so this
        // stays prose rather than becoming an image that cannot load.
        let blocks = parse("![x](javascript:alert(1))");
        assert!(matches!(blocks.as_slice(), [NoteBlock::Paragraph { .. }]));
        // And a line with anything else on it is prose, not a block.
        let blocks = parse("look: ![x](https://example.com/a.png)");
        assert!(matches!(blocks.as_slice(), [NoteBlock::Paragraph { .. }]));
    }

    #[test]
    fn numbered_lists_and_a_switch_of_marker() {
        let blocks = parse("1. one\n2) two\n   wrapped\n- bullet\n");
        assert_eq!(
            blocks,
            vec![
                NoteBlock::List {
                    ordered: true,
                    items: vec![vec![text("one")], vec![text("two wrapped")]],
                },
                NoteBlock::List {
                    ordered: false,
                    items: vec![vec![text("bullet")]],
                },
            ]
        );
        // A year starting a sentence is not a list, nor is a number with
        // no space after its dot.
        assert!(matches!(parse("2026.").as_slice(), [NoteBlock::Paragraph { .. }]));
        assert!(matches!(parse("3.14 is pi").as_slice(), [NoteBlock::Paragraph { .. }]));
    }

    #[test]
    fn a_quote_holds_blocks_of_its_own() {
        let blocks = parse("before\n> **Note**\n> - a\n>\n> said\nafter");
        assert_eq!(
            blocks,
            vec![
                NoteBlock::Paragraph { spans: vec![text("before")] },
                NoteBlock::Quote {
                    blocks: vec![
                        NoteBlock::Paragraph {
                            spans: vec![NoteSpan::Strong { text: "Note".into() }],
                        },
                        NoteBlock::List {
                            ordered: false,
                            items: vec![vec![text("a")]],
                        },
                        NoteBlock::Paragraph { spans: vec![text("said")] },
                    ],
                },
                NoteBlock::Paragraph { spans: vec![text("after")] },
            ]
        );
    }

    #[test]
    fn quotes_nest_but_not_without_end() {
        assert_eq!(
            parse("> > a"),
            vec![NoteBlock::Quote {
                blocks: vec![NoteBlock::Quote {
                    blocks: vec![NoteBlock::Paragraph { spans: vec![text("a")] }],
                }],
            }]
        );
        // Past the limit the rest is text, not a stack overflow.
        let mut blocks = parse(&">".repeat(100_000));
        for _ in 0..MAX_QUOTE_DEPTH {
            let [NoteBlock::Quote { blocks: inner }] = blocks.as_slice() else {
                panic!("expected a quote, got {blocks:?}");
            };
            blocks = inner.clone();
        }
        assert!(matches!(blocks.as_slice(), [NoteBlock::Paragraph { .. }]));
    }

    #[test]
    fn a_fence_keeps_lists_and_quotes_literal() {
        let blocks = parse("```\n> not a quote\n1. not a list\n```");
        assert_eq!(
            blocks,
            vec![NoteBlock::Code {
                text: "> not a quote\n1. not a list".into(),
                lang: None,
            }]
        );
    }

    #[test]
    fn a_local_image_stands_as_its_alt_text() {
        assert_eq!(
            parse("![the layout](./docs/layout.png)"),
            vec![NoteBlock::Paragraph { spans: vec![text("the layout")] }]
        );
        // With no alt there is nothing to show.
        assert!(parse("![](./a.png)").is_empty());
    }

    fn temp_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("mirador-notes-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn reads_a_markdown_file() {
        let path = temp_file("ok.md", b"# Plan\n\n1. first\n");
        let blocks = read_file(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(blocks.unwrap().len(), 2);
    }

    #[test]
    fn refuses_what_is_not_a_readable_markdown_file() {
        assert!(read_file(Path::new("README.md")).unwrap_err().contains("absolute"));

        let big = temp_file("big.md", &vec![b'a'; MAX_FILE_BYTES as usize + 1]);
        let err = read_file(&big).unwrap_err();
        std::fs::remove_file(&big).unwrap();
        assert!(err.contains("larger than"), "{err}");

        let binary = temp_file("bin.md", &[0xff, 0xfe, 0x00]);
        let err = read_file(&binary).unwrap_err();
        std::fs::remove_file(&binary).unwrap();
        assert!(err.contains("UTF-8"), "{err}");

        // A device reports a size of 0 and never ends; the cap still holds.
        #[cfg(unix)]
        assert!(read_file(Path::new("/dev/zero")).unwrap_err().contains("larger than"));

        let missing = std::env::temp_dir().join("mirador-notes-no-such-file.md");
        assert!(read_file(&missing).is_err());
    }

    fn git(dir: &Path, args: &[&str]) {
        let output = crate::proc::command("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git must be installed to run this test");
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    }

    fn fresh_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mirador-notes-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs/deep")).unwrap();
        for f in ["README.md", "notes.markdown", "main.rs", "docs/plan.md", "docs/deep/x.MD"] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        dir
    }

    fn relative(listed: &MarkdownFiles) -> Vec<String> {
        listed
            .files
            .iter()
            .map(|f| Path::new(f).strip_prefix(&listed.root).unwrap().to_string_lossy().replace('\\', "/"))
            .collect()
    }

    #[test]
    fn lists_a_repositorys_markdown_but_not_what_it_ignores() {
        let dir = fresh_dir("repo");
        git(&dir, &["init", "-q"]);
        std::fs::create_dir_all(dir.join("node_modules/pkg")).unwrap();
        std::fs::write(dir.join("node_modules/pkg/README.md"), "x").unwrap();
        std::fs::write(dir.join(".gitignore"), "node_modules/\n").unwrap();
        git(&dir, &["add", "README.md"]);

        // Asked from a subdirectory: the whole repository is listed.
        let listed = list_files(&dir.join("docs")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(listed.root, dir.to_string_lossy());
        // Tracked and untracked, shallowest first, ignored left out.
        assert_eq!(relative(&listed), ["README.md", "notes.markdown", "docs/plan.md", "docs/deep/x.MD"]);
        assert!(listed.files.iter().all(|f| Path::new(f).is_absolute()));
    }

    #[test]
    fn outside_a_repository_lists_only_the_directorys_own_files() {
        let dir = fresh_dir("plain");
        let listed = list_files(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        // temp_dir is no repository here, or this test would see one.
        assert_eq!(listed.root, dir.to_string_lossy());
        assert_eq!(relative(&listed), ["README.md", "notes.markdown"]);
    }

    #[test]
    fn empty_input_is_no_blocks() {
        assert!(parse("").is_empty());
        assert!(parse("\n\n\n").is_empty());
    }
}
