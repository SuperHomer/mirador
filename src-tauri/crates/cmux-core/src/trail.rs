//! What an agent did, turn by turn: each prompt, and the tool calls it made
//! for it, summed up — a command and how it ended, a path, a pattern.
//!
//! Read from the agent's own transcript (see `transcript`, whose tolerance
//! this shares: every field optional, a line that does not parse skipped),
//! and never copied out of it. A trail holds only the summaries; one call's
//! full input and output are read from the transcript when asked for, by
//! [`action_detail`], for as long as the transcript exists.
//!
//! A subagent's work is in its own transcript (`transcript::subagent_dir`),
//! not here: its `Agent` call is one action of the session's trail.

use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;

use cmux_protocol::{ActionDetail, ActionKind, ActionStatus, TrailAction, TrailTurn};
use serde_json::Value;

use crate::transcript::{parse_timestamp, text, Feed};

/// The longest a summary or a prompt's line gets, in characters.
const SUMMARY_CHARS: usize = 160;
/// The longest a turn's whole prompt is kept: a pasted document can be
/// far longer, and the trail is read every few seconds.
const PROMPT_CHARS: usize = 8000;
/// How much of a result a trail keeps: its first lines, each shortened.
const OUTPUT_LINES: usize = 5;
const OUTPUT_LINE_CHARS: usize = 200;

/// A Claude Code transcript, added up into turns.
#[derive(Debug, Default)]
pub struct Trail {
    turns: Vec<TrailTurn>,
    /// Calls awaiting their result: where their action is.
    pending: HashMap<String, (usize, usize)>,
}

impl Feed for Trail {
    fn feed(&mut self, line: &str) {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            return;
        };
        // Claude Code's own lines (caveats, injected context), and a
        // subagent's in transcripts that kept them inline, are not the
        // agent's conversation.
        if entry["isMeta"].as_bool() == Some(true) || entry["isSidechain"].as_bool() == Some(true) {
            return;
        }
        let ms = entry["timestamp"].as_str().and_then(parse_timestamp);
        match entry["type"].as_str() {
            Some("assistant") => {
                self.assistant(&entry["message"], ms);
                if let Some(turn) = self.turns.last_mut() {
                    stamp(turn, ms);
                }
            }
            Some("user") => self.user(&entry, ms),
            _ => {}
        }
    }
}

impl Trail {
    pub fn turns(&self) -> &[TrailTurn] {
        &self.turns
    }

    /// The call the agent is waiting on: its latest one still without a
    /// result. What a permission prompt is about, since Claude Code asks
    /// before it runs the call and writes the result after.
    pub fn asking(&self) -> Option<cmux_protocol::Asking> {
        let action = self
            .turns
            .iter()
            .flat_map(|t| &t.actions)
            .rfind(|a| a.status == ActionStatus::Running)?;
        Some(cmux_protocol::Asking { tool: action.tool.clone(), summary: action.summary.clone() })
    }

    fn assistant(&mut self, message: &Value, ms: Option<u64>) {
        let blocks = message["content"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        for block in blocks.iter().filter(|b| b["type"] == "tool_use") {
            let Some(id) = text(&block["id"]) else { continue };
            let tool = text(&block["name"]).unwrap_or_default();
            if self.turns.is_empty() {
                self.turns.push(turn(None, ms));
            }
            let t = self.turns.len() - 1;
            let actions = &mut self.turns[t].actions;
            self.pending.insert(id.clone(), (t, actions.len()));
            actions.push(TrailAction {
                summary: summarize(&tool, &block["input"]),
                kind: action_kind(&tool),
                id,
                tool,
                started_ms: ms,
                duration_ms: None,
                status: ActionStatus::Running,
                exit_code: None,
                output: String::new(),
            });
        }
    }

    fn user(&mut self, entry: &Value, ms: Option<u64>) {
        let content = &entry["message"]["content"];
        let blocks = content.as_array().map(Vec::as_slice).unwrap_or(&[]);
        let mut results = false;
        for block in blocks.iter().filter(|b| b["type"] == "tool_result") {
            results = true;
            let Some((t, a)) = block["tool_use_id"].as_str().and_then(|id| self.pending.remove(id)) else {
                continue;
            };
            // Its own turn's, which a late result is not the latest of.
            stamp(&mut self.turns[t], ms);
            let action = &mut self.turns[t].actions[a];
            let error = block["is_error"].as_bool() == Some(true);
            let output = result_text(&block["content"]);
            action.status = if error { ActionStatus::Error } else { ActionStatus::Ok };
            action.duration_ms = ms.zip(action.started_ms).map(|(end, start)| end.saturating_sub(start));
            if error && action.tool == "Bash" {
                action.exit_code = exit_code(&output);
            }
            action.output = head(&output);
        }
        if results {
            return;
        }
        // Not a tool's result: a prompt, or the note an interruption leaves.
        let prompt = match content {
            Value::String(s) => Some(s.as_str()),
            _ => blocks.iter().find(|b| b["type"] == "text").and_then(|b| b["text"].as_str()),
        };
        let Some(prompt) = prompt else { return };
        // A call still awaiting its result when the agent moves on never
        // gets one, unless it comes late — which then wins.
        for &(t, a) in self.pending.values() {
            self.turns[t].actions[a].status = ActionStatus::Interrupted;
        }
        if !prompt.starts_with("[Request interrupted") {
            self.turns.push(turn(Some(prompt), ms));
        } else if let Some(turn) = self.turns.last_mut() {
            stamp(turn, ms);
        }
    }
}

pub(crate) fn turn(prompt: Option<&str>, ms: Option<u64>) -> TrailTurn {
    TrailTurn {
        prompt: prompt.map(one_line),
        text: prompt.map(|p| shorten(p.trim(), PROMPT_CHARS)),
        started_ms: ms,
        last_ms: ms,
        actions: Vec::new(),
    }
}

/// Moves a turn's latest entry to `ms`, when the entry has one.
pub(crate) fn stamp(turn: &mut TrailTurn, ms: Option<u64>) {
    if ms.is_some() {
        turn.last_ms = ms;
    }
}

/// What sort of thing a tool does, by its name: Claude Code's tools, and
/// Codex's calls and items (`codex_trail`), and the kind an Extension item
/// says it is (`web_search`).
pub fn action_kind(tool: &str) -> ActionKind {
    match tool {
        "Read" | "Grep" | "Glob" | "LS" | "WebFetch" | "WebSearch" | "ImageView" | "view_image" | "web_search"
        | "image_view" => ActionKind::Read,
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" | "apply_patch" | "FileChange" | "file_change" => {
            ActionKind::Edit
        }
        "Bash" | "shell" | "local_shell_call" | "exec_command" | "CommandExecution" | "command_execution" => {
            ActionKind::Run
        }
        _ => ActionKind::Other,
    }
}

/// What a call is about, in a line: the field that says it for the tools
/// that have one, and otherwise the tool and its first string input
/// (by key order, the same in every build).
pub(crate) fn summarize(tool: &str, input: &Value) -> String {
    let field = match tool {
        "Bash" => "command",
        "Edit" | "Write" | "MultiEdit" | "Read" => "file_path",
        "NotebookEdit" => "notebook_path",
        "Grep" | "Glob" => "pattern",
        "WebFetch" => "url",
        "Agent" | "Task" => "description",
        _ => "",
    };
    if let Some(s) = text(&input[field]) {
        return one_line(&s);
    }
    let mut strings: Vec<(&String, &str)> = input
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| Some((k, v.as_str().filter(|s| !s.is_empty())?)))
        .collect();
    strings.sort();
    match strings.first() {
        Some((_, s)) => one_line(&format!("{tool}: {s}")),
        None => tool.to_string(),
    }
}

/// A result's text: a string, or its text blocks, an image as `[image]`.
fn result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| match b["type"].as_str() {
                Some("text") => b["text"].as_str().map(str::to_string),
                Some("image") => Some("[image]".to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// A failed command's exit code: Claude Code starts its result with
/// `Exit code N`.
fn exit_code(output: &str) -> Option<i32> {
    output.strip_prefix("Exit code ")?.split_whitespace().next()?.parse().ok()
}

/// The first line of `s`, shortened; `…` says something was left out.
pub(crate) fn one_line(s: &str) -> String {
    let s = s.trim();
    let first = s.lines().next().unwrap_or("");
    let cut = shorten(first, SUMMARY_CHARS);
    if cut.len() < s.len() && !cut.ends_with('…') {
        format!("{cut}…")
    } else {
        cut
    }
}

/// The first lines of a result, each shortened.
pub(crate) fn head(s: &str) -> String {
    let lines: Vec<String> = s
        .trim_end()
        .lines()
        .take(OUTPUT_LINES)
        .map(|l| shorten(l, OUTPUT_LINE_CHARS))
        .collect();
    let mut out = lines.join("\n");
    if s.trim_end().lines().nth(OUTPUT_LINES).is_some() {
        out.push_str("\n…");
    }
    out
}

fn shorten(s: &str, chars: usize) -> String {
    match s.char_indices().nth(chars) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

/// What a view is sent of a trail: some of its turns, oldest first, and
/// how many it was not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Window {
    pub turns: Vec<TrailTurn>,
    /// Turns before the first one sent.
    pub older: usize,
    /// Turns between the first one sent — a card's, older than the latest
    /// — and the latest; 0 when they follow on.
    pub gap: usize,
}

/// The turns a view asks for: the last `limit` (all, without one), and
/// the turn a card links to — the first begun at or after `since_ms` —
/// when it is older than those, alone before them: never more than
/// `limit + 1` turns, however old the card. Asking for a larger `limit`
/// pages back from the latest, closing the gap.
pub fn window(turns: &[TrailTurn], limit: Option<u32>, since_ms: Option<u64>) -> Window {
    let latest = limit.map_or(0, |n| turns.len().saturating_sub(n as usize));
    let target = since_ms
        .and_then(|since| turns.iter().position(|t| t.started_ms.is_some_and(|s| s >= since)))
        .filter(|&i| i < latest);
    match target {
        Some(t) => Window {
            turns: std::iter::once(&turns[t]).chain(&turns[latest..]).cloned().collect(),
            older: t,
            gap: latest - t - 1,
        },
        None => Window { turns: turns[latest..].to_vec(), older: latest, gap: 0 },
    }
}

/// What a trail answer is a reading of: the file, how far into it was
/// read, and the window asked for. Any line read since — a result landing
/// at the same instant as its call — changes it, so a view can skip an
/// answer whose version it has drawn already.
pub fn version(path: &Path, consumed: u64, limit: Option<u32>, since_ms: Option<u64>) -> String {
    format!("{}:{consumed}:{limit:?}:{since_ms:?}", path.display())
}

/// One call's full input and output, read from the transcript at `path`:
/// absent when the transcript is gone or holds no call `id`.
/// How many actions failed in the latest of `turns`: what a card in Doing
/// says about the turn its agent is in.
pub fn failing(turns: &[TrailTurn]) -> u32 {
    turns.last().map_or(0, |t| t.actions.iter().filter(|a| a.status == ActionStatus::Error).count() as u32)
}

pub fn action_detail(path: &Path, id: &str) -> Option<ActionDetail> {
    let file = std::fs::File::open(path).ok()?;
    let mut detail: Option<ActionDetail> = None;
    for line in std::io::BufReader::new(file).lines() {
        let Ok(line) = line else { break };
        // Most lines are not about this call; skip them unparsed.
        if !line.contains(id) {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(&line) else { continue };
        let blocks = entry["message"]["content"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        for block in blocks {
            match block["type"].as_str() {
                Some("tool_use") if block["id"] == id => {
                    detail = Some(ActionDetail {
                        id: id.to_string(),
                        tool: text(&block["name"]).unwrap_or_default(),
                        input: serde_json::to_string_pretty(&block["input"]).unwrap_or_default(),
                        output: None,
                        is_error: false,
                    });
                }
                Some("tool_result") if block["tool_use_id"] == id => {
                    if let Some(d) = detail.as_mut() {
                        d.output = Some(result_text(&block["content"]));
                        d.is_error = block["is_error"].as_bool() == Some(true);
                        return detail;
                    }
                }
                _ => {}
            }
        }
    }
    detail
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trail(lines: &[&str]) -> Vec<TrailTurn> {
        let mut trail = Trail::default();
        for line in lines {
            trail.feed(line);
        }
        trail.turns
    }

    fn prompt(text: &str, at: &str) -> String {
        format!(r#"{{"type":"user","timestamp":"2026-10-25T09:00:{at}.000Z","message":{{"role":"user","content":"{text}"}}}}"#)
    }

    fn call(id: &str, name: &str, input: &str, at: &str) -> String {
        format!(r#"{{"type":"assistant","timestamp":"2026-10-25T09:00:{at}.000Z","message":{{"id":"m-{id}","role":"assistant","content":[{{"type":"tool_use","id":"{id}","name":"{name}","input":{input}}}]}}}}"#)
    }

    fn result(id: &str, content: &str, error: bool, at: &str) -> String {
        format!(r#"{{"type":"user","timestamp":"2026-10-25T09:00:{at}.000Z","message":{{"role":"user","content":[{{"tool_use_id":"{id}","type":"tool_result","content":{content},"is_error":{error}}}]}},"toolUseResult":{{}}}}"#)
    }

    fn asking(lines: &[&str]) -> Option<(String, String)> {
        let mut trail = Trail::default();
        for line in lines {
            trail.feed(line);
        }
        trail.asking().map(|a| (a.tool, a.summary))
    }

    #[test]
    fn sorts_tools_into_reads_edits_and_runs() {
        for tool in ["Read", "Grep", "Glob", "LS", "WebFetch", "WebSearch", "ImageView", "view_image", "web_search"] {
            assert_eq!(action_kind(tool), ActionKind::Read, "{tool}");
        }
        for tool in ["Edit", "MultiEdit", "Write", "NotebookEdit", "apply_patch", "FileChange"] {
            assert_eq!(action_kind(tool), ActionKind::Edit, "{tool}");
        }
        for tool in ["Bash", "shell", "local_shell_call", "exec_command", "CommandExecution"] {
            assert_eq!(action_kind(tool), ActionKind::Run, "{tool}");
        }
        for tool in ["Agent", "TodoWrite", "McpToolCall", "mcp__github__search", ""] {
            assert_eq!(action_kind(tool), ActionKind::Other, "{tool}");
        }
        // As a trail reads them.
        let turns = trail(&[&prompt("Go", "00"), &call("t1", "Edit", r#"{"file_path":"/a.rs"}"#, "01")]);
        assert_eq!(turns[0].actions[0].kind, ActionKind::Edit);
    }

    #[test]
    fn counts_the_failures_of_the_latest_turn_only() {
        let failed = |id: &str, at: &str| result(id, r#""Exit code 1""#, true, at);
        let turns = trail(&[
            &prompt("First", "00"),
            &call("a", "Bash", r#"{"command":"false"}"#, "01"),
            &failed("a", "02"),
            &prompt("Second", "03"),
            &call("b", "Bash", r#"{"command":"false"}"#, "04"),
            &failed("b", "05"),
            &call("c", "Bash", r#"{"command":"true"}"#, "06"),
            &result("c", r#""ok""#, false, "07"),
            &call("d", "Bash", r#"{"command":"false"}"#, "08"),
            &failed("d", "09"),
        ]);
        assert_eq!(failing(&turns), 2);
        assert_eq!(failing(&[]), 0);
    }

    #[test]
    fn asks_about_the_latest_call_without_a_result() {
        let ask = |tool: &str, summary: &str| Some((tool.to_string(), summary.to_string()));
        // A command waiting to run.
        let bash = call("t1", "Bash", r#"{"command":"rm -rf build","description":"Clean"}"#, "01");
        assert_eq!(asking(&[&prompt("Clean up", "00"), &bash]), ask("Bash", "rm -rf build"));
        // An edit waiting to be made: its file.
        let edit = call("t2", "Edit", r#"{"file_path":"/src/app.rs","old_string":"a","new_string":"b"}"#, "02");
        assert_eq!(asking(&[&prompt("Fix it", "00"), &edit]), ask("Edit", "/src/app.rs"));
        // Answered already: nothing is waiting.
        assert_eq!(asking(&[&prompt("Clean up", "00"), &bash, &result("t1", r#""ok""#, false, "03")]), None);
        assert_eq!(asking(&[&prompt("Hello", "00")]), None);
        // Several calls in one message, one of them unanswered: that one.
        let both = r#"{"type":"assistant","timestamp":"2026-10-25T09:00:04.000Z","message":{"id":"m-x","role":"assistant","content":[{"type":"tool_use","id":"r1","name":"Read","input":{"file_path":"/a.md"}},{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/b.md","content":"x"}}]}}"#;
        assert_eq!(
            asking(&[&prompt("Copy it", "00"), both, &result("r1", r#""text""#, false, "05")]),
            ask("Write", "/b.md")
        );
    }

    #[test]
    fn pairs_each_call_with_its_result_turn_by_turn() {
        let turns = trail(&[
            &prompt(r"Fix the build\nIt fails on main", "00"),
            &call("t1", "Bash", r#"{"command":"cargo build","description":"Build"}"#, "01"),
            &call("t2", "Read", r#"{"file_path":"/w/src/lib.rs"}"#, "02"),
            &result("t2", r#""fn main() {}""#, false, "02"),
            &result("t1", r#""Exit code 101\nerror[E0425]: cannot find value""#, true, "04"),
            &call("t3", "Edit", r#"{"file_path":"/w/src/lib.rs","old_string":"a","new_string":"b"}"#, "05"),
            &result("t3", r#""The file has been updated.""#, false, "05"),
            r#"{"type":"assistant","timestamp":"2026-10-25T09:00:06.000Z","message":{"content":[{"type":"text","text":"Fixed."}],"stop_reason":"end_turn"}}"#,
            &prompt("Now test it", "10"),
            &call("t4", "Bash", r#"{"command":"cargo test"}"#, "11"),
        ]);
        assert_eq!(turns.len(), 2);
        let first = &turns[0];
        assert_eq!(first.prompt.as_deref(), Some("Fix the build…"));
        assert_eq!(first.text.as_deref(), Some("Fix the build\nIt fails on main"));
        assert_eq!(first.started_ms, Some(1_792_918_800_000));
        assert_eq!(first.last_ms, Some(1_792_918_806_000));
        let summed: Vec<(&str, &str, ActionStatus, Option<u64>)> = first
            .actions
            .iter()
            .map(|a| (a.tool.as_str(), a.summary.as_str(), a.status, a.duration_ms))
            .collect();
        assert_eq!(
            summed,
            [
                ("Bash", "cargo build", ActionStatus::Error, Some(3000)),
                ("Read", "/w/src/lib.rs", ActionStatus::Ok, Some(0)),
                ("Edit", "/w/src/lib.rs", ActionStatus::Ok, Some(0)),
            ]
        );
        assert_eq!(first.actions[0].exit_code, Some(101));
        assert_eq!(first.actions[0].output, "Exit code 101\nerror[E0425]: cannot find value");
        // No result yet: still running.
        assert_eq!(turns[1].actions[0].status, ActionStatus::Running);
        assert_eq!(turns[1].actions[0].duration_ms, None);
    }

    #[test]
    fn sums_up_each_tool_by_what_says_it() {
        let summary = |name: &str, input: &str| {
            let turns = trail(&[&prompt("go", "00"), &call("t", name, input, "01")]);
            turns[0].actions[0].summary.clone()
        };
        assert_eq!(summary("Write", r#"{"file_path":"/w/a.md","content":"x"}"#), "/w/a.md");
        assert_eq!(summary("MultiEdit", r#"{"file_path":"/w/b.rs","edits":[]}"#), "/w/b.rs");
        assert_eq!(summary("NotebookEdit", r#"{"notebook_path":"/w/n.ipynb"}"#), "/w/n.ipynb");
        assert_eq!(summary("Grep", r#"{"pattern":"fn main","path":"/w"}"#), "fn main");
        assert_eq!(summary("Glob", r#"{"pattern":"**/*.rs"}"#), "**/*.rs");
        assert_eq!(summary("WebFetch", r#"{"url":"https://example.com","prompt":"x"}"#), "https://example.com");
        assert_eq!(summary("Agent", r#"{"description":"Map the crates","prompt":"…"}"#), "Map the crates");
        assert_eq!(summary("Task", r#"{"description":"Old name","prompt":"…"}"#), "Old name");
        // Anything else: the tool and its first string, by key.
        assert_eq!(summary("Skill", r#"{"skill":"review","args":"--fast"}"#), "Skill: --fast");
        assert_eq!(summary("mcp__x__tabs", r#"{"tabId":3}"#), "mcp__x__tabs");
        // A long, many-line command: its first line, shortened.
        let long = "x".repeat(300);
        let s = summary("Bash", &format!(r#"{{"command":"{long}\necho done"}}"#));
        assert_eq!(s.chars().count(), SUMMARY_CHARS + 1);
        assert!(s.ends_with('…'));
        assert_eq!(summary("Bash", r#"{"command":"cd /w &&\nmake"}"#), "cd /w &&…");
    }

    #[test]
    fn keeps_only_the_first_lines_of_a_result() {
        let many: Vec<String> = (1..=9).map(|n| n.to_string()).collect();
        let blocks = r#"[{"type":"text","text":"page"},{"type":"image","source":{}},{"type":"tool_reference","tool_name":"x"}]"#;
        let turns = trail(&[
            &prompt("go", "00"),
            &call("a", "Bash", r#"{"command":"seq 9"}"#, "01"),
            &result("a", &format!("{:?}", many.join("\n")), false, "01"),
            &call("b", "mcp__x__shot", r#"{}"#, "02"),
            &result("b", blocks, false, "02"),
            &call("c", "Read", r#"{"file_path":"/w/wide"}"#, "03"),
            &result("c", &format!("{:?}", "y".repeat(500)), false, "03"),
        ]);
        let out: Vec<&str> = turns[0].actions.iter().map(|a| a.output.as_str()).collect();
        assert_eq!(out[0], "1\n2\n3\n4\n5\n…");
        assert_eq!(out[1], "page\n[image]");
        assert_eq!(out[2].chars().count(), OUTPUT_LINE_CHARS + 1);
    }

    #[test]
    fn starts_a_turn_only_at_a_prompt() {
        let interrupted = r#"{"type":"user","message":{"content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#;
        let meta = r#"{"type":"user","isMeta":true,"message":{"content":"<caveat>"}}"#;
        let sidechain = r#"{"type":"user","isSidechain":true,"message":{"content":"a subagent's prompt"}}"#;
        let turns = trail(&[
            // A transcript that starts mid-conversation.
            &call("a", "Read", r#"{"file_path":"/w/x"}"#, "00"),
            &prompt("first", "01"),
            meta,
            sidechain,
            &call("b", "Bash", r#"{"command":"sleep 99"}"#, "02"),
            interrupted,
            r#"{"type":"user","message":{"content":[{"type":"text","text":"second"}]}}"#,
        ]);
        let prompts: Vec<Option<&str>> = turns.iter().map(|t| t.prompt.as_deref()).collect();
        assert_eq!(prompts, [None, Some("first"), Some("second")]);
        // Left without a result when the agent moved on.
        assert_eq!(turns[0].actions[0].status, ActionStatus::Interrupted);
        assert_eq!(turns[1].actions[0].status, ActionStatus::Interrupted);
        // A result that comes late still wins.
        let mut t = Trail::default();
        for line in [&prompt("go", "00"), &call("a", "Bash", "{}", "01"), &prompt("next", "02"), &result("a", r#""ok""#, false, "03")] {
            t.feed(line);
        }
        assert_eq!(t.turns()[0].actions[0].status, ActionStatus::Ok);
        // And moves on its own turn's time, not the newer one's.
        assert_eq!(t.turns()[0].last_ms, Some(1_792_918_803_000));
        assert_eq!(t.turns()[1].last_ms, Some(1_792_918_802_000));
    }

    #[test]
    fn a_window_is_the_latest_turns_and_a_card_s_turn() {
        let turns: Vec<TrailTurn> = (0..10).map(|i| turn(Some("go"), Some(i * 1000))).collect();
        let seen = |w: Window| (w.turns.iter().map(|t| t.started_ms.unwrap() / 1000).collect::<Vec<_>>(), w.older, w.gap);
        assert_eq!(seen(window(&turns, None, None)), ((0..10).collect(), 0, 0));
        assert_eq!(seen(window(&turns, Some(3), None)), (vec![7, 8, 9], 7, 0));
        assert_eq!(seen(window(&turns, Some(30), None)), ((0..10).collect(), 0, 0));
        // A card given before the latest three: its turn, then a gap.
        assert_eq!(seen(window(&turns, Some(3), Some(4500))), (vec![5, 7, 8, 9], 5, 1));
        // Right before them: no gap.
        assert_eq!(seen(window(&turns, Some(3), Some(6000))), (vec![6, 7, 8, 9], 6, 0));
        // Within the latest, or nothing begun since: the latest alone.
        assert_eq!(seen(window(&turns, Some(3), Some(8000))), (vec![7, 8, 9], 7, 0));
        assert_eq!(seen(window(&turns, Some(2), Some(99_000))), (vec![8, 9], 8, 0));
        assert_eq!(seen(window(&[], Some(3), Some(0))), (vec![], 0, 0));
    }

    #[test]
    fn an_old_card_s_window_stays_bounded() {
        let turns: Vec<TrailTurn> = (0..200).map(|i| turn(Some("go"), Some(i * 1000))).collect();
        let w = window(&turns, Some(20), Some(1000));
        assert!(w.turns.len() <= 21, "{}", w.turns.len());
        assert_eq!(w.turns[0].started_ms, Some(1000), "the card's turn");
        assert_eq!(w.turns[1].started_ms, Some(180_000), "then the latest twenty");
        assert_eq!((w.older, w.gap), (1, 178));
        // Paging back from the latest closes the gap, still holding it.
        let w = window(&turns, Some(40), Some(1000));
        assert_eq!((w.turns.len(), w.older, w.gap), (41, 1, 158));
    }

    #[test]
    fn skips_what_does_not_parse() {
        let turns = trail(&[
            "",
            "{",
            "null",
            r#"{"type":"assistant","message":"x"}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Bash"}]}}"#,
            r#"{"type":"user"}"#,
            &result("nobody", r#""orphan""#, false, "00"),
            r#"{"type":"api-request-blob","hash":"h","message":{"content":[]}}"#,
        ]);
        assert!(turns.is_empty());
    }

    #[test]
    fn reads_one_action_in_full_from_the_transcript() {
        let dir = std::env::temp_dir().join(format!("mira-trail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s1.jsonl");
        let many: String = (1..=50).map(|n| format!("{n}\n")).collect();
        let lines = [
            prompt("go", "00"),
            call("a", "Bash", r#"{"command":"seq 50"}"#, "01"),
            call("b", "Bash", r#"{"command":"sleep 9"}"#, "02"),
            result("a", &format!("{many:?}"), false, "03"),
        ];
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();

        let a = action_detail(&path, "a").unwrap();
        assert_eq!((a.tool.as_str(), a.is_error), ("Bash", false));
        assert_eq!(a.input, "{\n  \"command\": \"seq 50\"\n}");
        assert_eq!(a.output.as_deref(), Some(many.as_str()));
        // No result yet.
        assert_eq!(action_detail(&path, "b").unwrap().output, None);
        assert_eq!(action_detail(&path, "zzz"), None);
        let _ = std::fs::remove_dir_all(&dir);
        // The transcript gone, so is the detail.
        assert_eq!(action_detail(&path, "a"), None);
    }

    #[test]
    fn a_cache_reads_a_trail_as_it_grows() {
        let dir = std::env::temp_dir().join(format!("mira-trail-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s1.jsonl");
        std::fs::write(&path, format!("{}\n{}\n", prompt("go", "00"), call("a", "Bash", "{}", "01"))).unwrap();
        let mut cache: crate::transcript::Cache<Trail> = Default::default();
        assert_eq!(cache.read(&path).unwrap().turns()[0].actions[0].status, ActionStatus::Running);
        let before = version(&path, cache.consumed(&path).unwrap(), Some(20), None);
        assert_eq!(cache.consumed(&path), Some(std::fs::metadata(&path).unwrap().len()));
        // Its result, at the very instant of the call: the same turn times.
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        std::io::Write::write_all(&mut f, format!("{}\n", result("a", r#""done""#, false, "01")).as_bytes()).unwrap();
        let turns = cache.read(&path).unwrap().turns().to_vec();
        assert_eq!(turns[0].actions[0].status, ActionStatus::Ok);
        assert_eq!(turns[0].last_ms, Some(1_792_918_801_000), "nothing for a fingerprint of times to see");
        assert_ne!(version(&path, cache.consumed(&path).unwrap(), Some(20), None), before);
        // The same reading asked for another window is another answer.
        assert_ne!(version(&path, cache.consumed(&path).unwrap(), Some(40), None), version(&path, cache.consumed(&path).unwrap(), Some(20), None));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
