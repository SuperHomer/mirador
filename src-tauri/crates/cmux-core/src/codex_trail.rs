//! What a Codex agent did, turn by turn, read from the rollout Codex
//! writes as it goes (`$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<time>-<id>.jsonl`)
//! into the same [`TrailTurn`]s as a Claude Code transcript (`trail`).
//!
//! Each line is `{timestamp, type, payload}`. A turn runs from an
//! `event_msg` `task_started` to its `task_complete` (or `turn_aborted`),
//! both naming its `turn_id`. What the agent did is in `item_completed`
//! events: a `CommandExecution` carries its command, exit code, output
//! and times, and the other tool items (`FileChange`, `McpToolCall`,
//! `WebSearch`, `ImageView`, …) theirs. Codex 0.163 runs its tools from a
//! script, the `exec` custom tool, so that call and its `wait` are only
//! the wrapper and are left out. Calls made as `function_call` or
//! `custom_tool_call` with any other name (what earlier versions wrote for
//! `shell` and `apply_patch`) are actions, paired with their `…_output` by
//! `call_id`. Nothing here is a documented format either: every field is
//! optional, and a line that does not parse is skipped.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use cmux_protocol::{ActionDetail, ActionStatus, TrailAction, TrailTurn, TranscriptSummary, TurnEnd};
use serde_json::Value;

use crate::trail::{head, one_line, stamp, summarize, turn};
use crate::transcript::{context_window, parse_timestamp, text, Feed};

/// Items that are the conversation, not something the agent did.
const NOT_ACTIONS: &[&str] =
    &["UserMessage", "AgentMessage", "Reasoning", "ContextCompaction", "HookPrompt", "EnteredReviewMode", "ExitedReviewMode"];
/// Code mode's own calls: the script its tools run in, and waiting on it.
const WRAPPERS: &[&str] = &["exec", "wait"];
/// What an item's result is in: not part of its input.
const OUTPUT_KEYS: &[&str] = &["aggregated_output", "result", "results", "error"];

/// A Codex rollout, added up into turns.
#[derive(Debug, Default)]
pub struct CodexTrail {
    turns: Vec<TrailTurn>,
    /// Turns by their Codex `turn_id`.
    by_id: HashMap<String, usize>,
    /// Calls awaiting their output: where their action is.
    pending: HashMap<String, (usize, usize)>,
}

impl Feed for CodexTrail {
    fn feed(&mut self, line: &str) {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            return;
        };
        let ms = entry["timestamp"].as_str().and_then(parse_timestamp);
        let p = &entry["payload"];
        match (entry["type"].as_str(), p["type"].as_str()) {
            (Some("event_msg"), Some("task_started")) => {
                let at = p["started_at"].as_u64().map(|s| s * 1000).or(ms);
                self.turns.push(turn(None, at));
                if let Some(id) = text(&p["turn_id"]) {
                    self.by_id.insert(id, self.turns.len() - 1);
                }
            }
            (Some("event_msg"), Some("task_complete" | "turn_aborted")) => {
                let Some(t) = self.turn_of(p) else { return };
                stamp(&mut self.turns[t], ms);
                // What it called and never heard back from, it never will.
                for &(pt, a) in self.pending.values() {
                    if pt == t {
                        self.turns[t].actions[a].status = ActionStatus::Interrupted;
                    }
                }
            }
            (Some("event_msg"), Some("user_message")) => self.prompt(p, text(&p["message"]), ms),
            (Some("event_msg"), Some("item_completed")) => self.item(p, ms),
            (Some("response_item"), Some("function_call" | "custom_tool_call" | "local_shell_call")) => self.call(p, ms),
            (Some("response_item"), Some(kind)) if kind.ends_with("_call_output") => self.output(p, ms),
            _ => {}
        }
    }
}

impl CodexTrail {
    pub fn turns(&self) -> &[TrailTurn] {
        &self.turns
    }

    /// The turn an entry names, else the latest one, else a new one: a
    /// rollout read from its middle, or one from before turn ids.
    fn turn_of(&mut self, p: &Value) -> Option<usize> {
        let named = p["turn_id"]
            .as_str()
            .or(p["internal_chat_message_metadata_passthrough"]["turn_id"].as_str())
            .and_then(|id| self.by_id.get(id).copied());
        named.or(self.turns.len().checked_sub(1))
    }

    fn turn_for_action(&mut self, p: &Value, ms: Option<u64>) -> usize {
        self.turn_of(p).unwrap_or_else(|| {
            self.turns.push(turn(None, ms));
            self.turns.len() - 1
        })
    }

    /// The first user message of a turn is its prompt.
    fn prompt(&mut self, p: &Value, prompt: Option<String>, ms: Option<u64>) {
        let Some(prompt) = prompt else { return };
        let t = self.turn_for_action(p, ms);
        let turn = &mut self.turns[t];
        if turn.prompt.is_none() {
            let full = crate::trail::turn(Some(&prompt), None);
            (turn.prompt, turn.text) = (full.prompt, full.text);
        }
        stamp(turn, ms);
    }

    fn item(&mut self, p: &Value, ms: Option<u64>) {
        let item = &p["item"];
        let Some(kind) = text(&item["type"]) else { return };
        if kind == "UserMessage" {
            let said: Vec<&str> = item["content"].as_array().into_iter().flatten().filter_map(|c| c["text"].as_str()).collect();
            return self.prompt(p, Some(said.join("\n")).filter(|s| !s.is_empty()), ms);
        }
        if NOT_ACTIONS.contains(&kind.as_str()) {
            return;
        }
        let Some(id) = text(&item["id"]) else { return };
        let started = p["started_at_ms"].as_u64();
        let duration = match (started, p["completed_at_ms"].as_u64()) {
            (Some(s), Some(e)) => Some(e.saturating_sub(s)),
            _ => item["duration"]["secs"].as_u64().map(|s| s * 1000 + item["duration"]["nanos"].as_u64().unwrap_or(0) / 1_000_000),
        };
        let exit_code = item["exit_code"].as_i64().and_then(|c| i32::try_from(c).ok());
        let failed = item_failed(item);
        let t = self.turn_for_action(p, ms);
        let turn = &mut self.turns[t];
        turn.actions.push(TrailAction {
            summary: item_summary(&kind, item),
            // An Extension says what it is in its own kind (`web_search`).
            kind: crate::trail::action_kind(match kind.as_str() {
                "Extension" => item["kind"].as_str().unwrap_or_default(),
                k => k,
            }),
            tool: kind,
            id,
            started_ms: started.or(ms),
            duration_ms: duration,
            status: if failed { ActionStatus::Error } else { ActionStatus::Ok },
            exit_code,
            output: head(&item_output(item)),
        });
        stamp(turn, ms);
    }

    fn call(&mut self, p: &Value, ms: Option<u64>) {
        let tool = text(&p["name"]).unwrap_or_else(|| p["type"].as_str().unwrap_or_default().to_string());
        if WRAPPERS.contains(&tool.as_str()) {
            return;
        }
        let Some(id) = text(&p["call_id"]).or_else(|| text(&p["id"])) else { return };
        let input = call_input(p);
        let t = self.turn_for_action(p, ms);
        let turn = &mut self.turns[t];
        self.pending.insert(id.clone(), (t, turn.actions.len()));
        turn.actions.push(TrailAction {
            summary: call_summary(&tool, &input),
            kind: crate::trail::action_kind(&tool),
            tool,
            id,
            started_ms: ms,
            duration_ms: None,
            status: ActionStatus::Running,
            exit_code: None,
            output: String::new(),
        });
        stamp(turn, ms);
    }

    fn output(&mut self, p: &Value, ms: Option<u64>) {
        let Some((t, a)) = p["call_id"].as_str().and_then(|id| self.pending.remove(id)) else {
            return;
        };
        stamp(&mut self.turns[t], ms);
        let action = &mut self.turns[t].actions[a];
        let (output, exit_code) = call_output(&p["output"]);
        action.duration_ms = ms.zip(action.started_ms).map(|(end, start)| end.saturating_sub(start));
        action.exit_code = exit_code;
        action.status = if exit_code.is_some_and(|c| c != 0) { ActionStatus::Error } else { ActionStatus::Ok };
        action.output = head(&output);
    }
}

/// What a Codex rollout adds up to, in the terms of a Claude Code
/// transcript's [`TranscriptSummary`], for the activity panel and the
/// agent status bar. Codex writes its own counts: each `token_count` event
/// carries the session's running `total_token_usage`, the last model
/// call's `last_token_usage`, and its `model_context_window`; each
/// `turn_context` the model. Its `input_tokens` include the cached ones,
/// and its `output_tokens` the reasoning.
#[derive(Debug, Default)]
pub struct CodexStats {
    model: Option<String>,
    tokens: u64,
    context: u64,
    window: Option<u64>,
    tool_calls: u32,
    started_ms: Option<u64>,
    last_ms: Option<u64>,
}

impl Feed for CodexStats {
    fn feed(&mut self, line: &str) {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            return;
        };
        if let Some(ms) = entry["timestamp"].as_str().and_then(parse_timestamp) {
            self.started_ms.get_or_insert(ms);
            self.last_ms = Some(ms);
        }
        let p = &entry["payload"];
        match (entry["type"].as_str(), p["type"].as_str()) {
            (Some("turn_context"), _) => {
                if let Some(model) = text(&p["model"]) {
                    self.model = Some(model);
                }
            }
            (Some("event_msg"), Some("token_count")) => self.token_count(&p["info"]),
            // Counted as the trail counts its actions.
            (Some("event_msg"), Some("item_completed")) => {
                let item = &p["item"];
                let kind = item["type"].as_str().unwrap_or("UserMessage");
                if kind != "UserMessage" && !NOT_ACTIONS.contains(&kind) && item["id"].is_string() {
                    self.tool_calls += 1;
                }
            }
            (Some("response_item"), Some("function_call" | "custom_tool_call" | "local_shell_call")) => {
                let tool = p["name"].as_str().unwrap_or_default();
                if !WRAPPERS.contains(&tool) {
                    self.tool_calls += 1;
                }
            }
            _ => {}
        }
    }
}

impl CodexStats {
    /// Fresh tokens are what a Claude transcript counts: input not read
    /// from the cache, plus output. The context is the last call's input,
    /// as a Claude transcript's is its last reply's.
    fn token_count(&mut self, info: &Value) {
        let n = |usage: &Value, key: &str| usage[key].as_u64().unwrap_or(0);
        let total = &info["total_token_usage"];
        if total.is_object() {
            self.tokens = n(total, "input_tokens").saturating_sub(n(total, "cached_input_tokens")) + n(total, "output_tokens");
        }
        let last = &info["last_token_usage"];
        if last.is_object() {
            self.context = n(last, "input_tokens");
        }
        if let Some(window) = info["model_context_window"].as_u64() {
            self.window = Some(window);
        }
    }

    /// The window Codex reported wins over the table's guess.
    pub fn summary(&self) -> TranscriptSummary {
        TranscriptSummary {
            model: self.model.clone(),
            tokens: self.tokens,
            context: self.context,
            context_window: self.window.or_else(|| self.model.as_deref().and_then(context_window)),
            tool_calls: self.tool_calls,
            // OpenAI's prices are not known here: its replies are real and
            // not in a cost, never counted as free.
            cost_usd: None,
            unpriced: self.tokens > 0,
            tasks: None,
            started_ms: self.started_ms,
            last_ms: self.last_ms,
        }
    }
}

/// A command as it was typed: Codex runs `["/bin/zsh", "-lc", "<script>"]`.
fn command_text(command: &Value) -> Option<String> {
    match command {
        Value::String(s) => Some(s.clone()),
        Value::Array(words) => {
            let words: Vec<&str> = words.iter().filter_map(Value::as_str).collect();
            match words.as_slice() {
                [_, flag, script] if matches!(*flag, "-lc" | "-c") => Some(script.to_string()),
                [] => None,
                _ => Some(words.join(" ")),
            }
        }
        _ => None,
    }
}

/// The files a patch touches, from its `*** Update/Add/Delete File:` lines.
fn patch_paths(patch: &str) -> Option<String> {
    let paths: Vec<&str> = patch
        .lines()
        .filter_map(|l| {
            ["*** Update File: ", "*** Add File: ", "*** Delete File: "]
                .iter()
                .find_map(|p| l.strip_prefix(p))
        })
        .map(str::trim)
        .collect();
    (!paths.is_empty()).then(|| paths.join(", "))
}

/// A command that is a patch (`apply_patch <<EOF …`) is summed up as one.
fn command_summary(command: &str) -> String {
    match patch_paths(command) {
        Some(paths) => one_line(&paths),
        None => one_line(command),
    }
}

fn item_summary(kind: &str, item: &Value) -> String {
    let said = match kind {
        "CommandExecution" => command_text(&item["command"]).map(|c| command_summary(&c)),
        // By path, as a map or a list.
        "FileChange" => {
            let paths: Vec<String> = match &item["changes"] {
                Value::Object(map) => map.keys().cloned().collect(),
                Value::Array(list) => list.iter().filter_map(|c| text(&c["path"])).collect(),
                _ => Vec::new(),
            };
            (!paths.is_empty()).then(|| one_line(&paths.join(", ")))
        }
        "ImageView" => text(&item["path"]).map(|p| one_line(&p)),
        "WebSearch" | "Extension" => text(&item["query"]).map(|q| one_line(&q)),
        "McpToolCall" => match (text(&item["server"]), text(&item["tool"])) {
            (Some(server), Some(tool)) => Some(format!("{server}.{tool}")),
            _ => None,
        },
        _ => None,
    };
    said.unwrap_or_else(|| {
        let mut rest = item.clone();
        if let Some(map) = rest.as_object_mut() {
            map.retain(|k, _| !matches!(k.as_str(), "id" | "type" | "status") && !OUTPUT_KEYS.contains(&k.as_str()));
        }
        summarize(kind, &rest)
    })
}

/// Whether an item says it failed: by its status, a non-zero exit code,
/// or an error it carries (an MCP call's, which keeps its status).
fn item_failed(item: &Value) -> bool {
    matches!(item["status"].as_str(), Some("failed" | "declined" | "error"))
        || item["exit_code"].as_i64().is_some_and(|c| c != 0)
        || item["error"].is_string()
        || item["error"].is_object()
}

/// What an item says came of it.
fn item_output(item: &Value) -> String {
    OUTPUT_KEYS
        .iter()
        .find_map(|k| match &item[*k] {
            Value::Null => None,
            Value::String(s) => Some(s.clone()),
            other => serde_json::to_string_pretty(other).ok(),
        })
        .unwrap_or_default()
}

/// A call's input: `function_call` arguments are a JSON string,
/// `custom_tool_call` input free text, `local_shell_call` an action.
fn call_input(p: &Value) -> Value {
    if let Some(args) = p["arguments"].as_str() {
        return serde_json::from_str(args).unwrap_or_else(|_| Value::String(args.to_string()));
    }
    if !p["input"].is_null() {
        return p["input"].clone();
    }
    p["action"].clone()
}

fn call_summary(tool: &str, input: &Value) -> String {
    if let Some(patch) = input.as_str().and_then(patch_paths).or_else(|| input["input"].as_str().and_then(patch_paths)) {
        return one_line(&patch);
    }
    let command = command_text(&input["command"]).or_else(|| text(&input["cmd"]));
    if let Some(command) = command.filter(|_| matches!(tool, "shell" | "exec_command" | "container.exec" | "local_shell_call")) {
        return command_summary(&command);
    }
    match input {
        Value::String(s) => one_line(&format!("{tool}: {s}")),
        _ => summarize(tool, input),
    }
}

/// An output's text and the exit code it reports: a string, a list of
/// `input_text` parts, or (in earlier versions) JSON with
/// `metadata.exit_code`; else a `Process exited with code N` line.
fn call_output(output: &Value) -> (String, Option<i32>) {
    let text = match output {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| match p["type"].as_str() {
                Some("input_image") => Some("[image]".to_string()),
                _ => p["text"].as_str().map(str::to_string),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    if let Ok(json) = serde_json::from_str::<Value>(&text) {
        if let Some(code) = json["metadata"]["exit_code"].as_i64() {
            let body = json["output"].as_str().unwrap_or_default().to_string();
            return (body, i32::try_from(code).ok());
        }
    }
    let code = text.lines().find_map(|l| {
        l.strip_prefix("Process exited with code ")
            .or_else(|| l.strip_prefix("Exit code: "))
            .and_then(|c| c.trim().parse().ok())
    });
    (text, code)
}

/// One action's full input and output, read from the rollout at `path`
/// by its item id or call id: absent when the rollout is gone or holds
/// no such action.
pub fn action_detail(path: &Path, id: &str) -> Option<ActionDetail> {
    let file = std::fs::File::open(path).ok()?;
    let mut detail: Option<ActionDetail> = None;
    for line in std::io::BufReader::new(file).lines() {
        let Ok(line) = line else { break };
        if !line.contains(id) {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(&line) else { continue };
        let p = &entry["payload"];
        let item = &p["item"];
        if p["type"] == "item_completed" && item["id"] == id {
            let mut input = item.clone();
            if let Some(map) = input.as_object_mut() {
                map.retain(|k, _| !OUTPUT_KEYS.contains(&k.as_str()));
            }
            let output = item_output(item);
            return Some(ActionDetail {
                id: id.to_string(),
                tool: text(&item["type"]).unwrap_or_default(),
                input: serde_json::to_string_pretty(&input).unwrap_or_default(),
                is_error: item_failed(item),
                output: Some(output),
            });
        }
        if p["call_id"] != id {
            continue;
        }
        match p["type"].as_str() {
            Some(kind) if kind.ends_with("_call_output") => {
                if let Some(d) = detail.as_mut() {
                    let (output, code) = call_output(&p["output"]);
                    d.is_error = code.is_some_and(|c| c != 0);
                    d.output = Some(output);
                    return detail;
                }
            }
            Some(_) => {
                let input = call_input(p);
                detail = Some(ActionDetail {
                    id: id.to_string(),
                    tool: text(&p["name"]).unwrap_or_default(),
                    input: match &input {
                        Value::String(s) => s.clone(),
                        other => serde_json::to_string_pretty(other).unwrap_or_default(),
                    },
                    output: None,
                    is_error: false,
                });
            }
            None => {}
        }
    }
    detail
}

/// Follows a rollout as it grows, for the ends of its turns: what Claude
/// Code's `Stop` hook tells of its turns, Codex writes only here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolloutTail {
    pub path: PathBuf,
    offset: u64,
    /// Turns already ended, so a turn ends once whatever its lines say.
    ended: HashSet<String>,
}

impl RolloutTail {
    /// From the end of `path` as it is now: the turns it holds already
    /// ended before Mirador looked, and are not logged (again).
    pub fn at_end(path: PathBuf) -> Self {
        let offset = std::fs::metadata(&path).map_or(0, |m| m.len());
        Self { path, offset, ended: HashSet::new() }
    }

    /// From the start of `path`: a session that began while its pane was
    /// followed, whose every turn is new work.
    pub fn from_start(path: PathBuf) -> Self {
        Self { path, offset: 0, ended: HashSet::new() }
    }

    /// The turns that ended in what was appended since the last read. A
    /// file that shrank was rewritten: it is followed from its new end.
    pub fn read(&mut self) -> Vec<TurnEnd> {
        let Ok(mut file) = std::fs::File::open(&self.path) else { return Vec::new() };
        let len = file.metadata().map_or(0, |m| m.len());
        if len <= self.offset {
            self.offset = len;
            return Vec::new();
        }
        let mut bytes = Vec::new();
        if file.seek(SeekFrom::Start(self.offset)).is_err() || file.take(len - self.offset).read_to_end(&mut bytes).is_err() {
            return Vec::new();
        }
        let (ends, used) = self.turn_ends(&bytes);
        self.offset += used;
        ends
    }

    /// The turn ends in `bytes`, appended at the offset, and how many of
    /// them were whole lines: a line still being written waits for the
    /// next read. A `task_complete` is a turn done; a `turn_aborted` (an
    /// interrupted turn) is one too, its agent waiting for its next prompt.
    fn turn_ends(&mut self, bytes: &[u8]) -> (Vec<TurnEnd>, u64) {
        let whole = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        let mut ends = Vec::new();
        for line in bytes[..whole].split(|&b| b == b'\n') {
            let Ok(entry) = serde_json::from_slice::<Value>(line) else { continue };
            let p = &entry["payload"];
            if entry["type"] != "event_msg" || !matches!(p["type"].as_str(), Some("task_complete" | "turn_aborted")) {
                continue;
            }
            // Bounded: turn ids are not reused, so forgetting old ones
            // only matters for a turn ending twice across that.
            if self.ended.len() > 1024 {
                self.ended.clear();
            }
            let turn = text(&p["turn_id"]).unwrap_or_default();
            if turn.is_empty() || self.ended.insert(turn) {
                ends.push(TurnEnd::Done);
            }
        }
        (ends, whole as u64)
    }
}

/// Whether a pane's rollout `path` is followed from its start rather than
/// its end, `before` being the one the pane followed until now. A pane's
/// first is history up to now; a newer one after it (`/new`) began while
/// the pane was followed, so a turn it ended before Mirador noticed is
/// still new. One no newer (`codex resume` reopening an old session) is
/// history again.
pub fn follow_from_start(before: Option<&Path>, path: &Path) -> bool {
    before.is_some_and(|before| path.file_name() > before.file_name())
}

/// The rollout among a process's open files: the one `rollout-*.jsonl`
/// under a `sessions` directory, else how many there were. None, or more
/// than one — a Codex that started another, or one thread per agent of a
/// multi-agent session — names none: no trail is better than another
/// agent's.
pub fn rollout_of(open: &[PathBuf]) -> Result<PathBuf, usize> {
    let mut found: Vec<&PathBuf> = open.iter().filter(|p| is_rollout(p)).collect();
    found.sort();
    found.dedup();
    match found.as_slice() {
        [one] => Ok((*one).clone()),
        _ => Err(found.len()),
    }
}

fn is_rollout(p: &Path) -> bool {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    name.starts_with("rollout-") && name.ends_with(".jsonl") && sessions_of(p).is_some()
}

/// The thread id a rollout is named after
/// (`rollout-<time>-<uuid>.jsonl`): what `codex resume` takes.
pub fn rollout_id(rollout: &Path) -> Option<String> {
    let stem = rollout.file_name()?.to_str()?.strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
    let id = stem.get(stem.len().checked_sub(36)?..)?;
    let groups: Vec<&str> = id.split('-').collect();
    let shape = groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12]);
    (shape && groups.iter().all(|g| g.chars().all(|c| c.is_ascii_hexdigit()))).then(|| id.to_ascii_lowercase())
}

/// The `sessions` directory a rollout is in.
pub fn sessions_of(rollout: &Path) -> Option<&Path> {
    rollout.ancestors().find(|a| a.file_name().is_some_and(|n| n == "sessions"))
}

/// The newest rollout under `sessions` (`YYYY/MM/DD/rollout-<time>-…`):
/// the last file of the last day. Names sort by when they were started,
/// so this is a few directory reads, not a walk.
pub fn newest_rollout(sessions: &Path) -> Option<PathBuf> {
    let last = |dir: &Path, dirs: bool| {
        std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| if dirs { p.is_dir() } else { is_rollout(p) })
            .max_by(|a, b| a.file_name().cmp(&b.file_name()))
    };
    let day = last(&last(&last(sessions, true)?, true)?, true)?;
    last(&day, false)
}

/// A Codex's rollout as last found: for the process `pid`, when `newest`
/// was the newest rollout there was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownRollout {
    pub pid: u32,
    pub path: PathBuf,
    pub newest: Option<PathBuf>,
}

/// Whether the Codex at `pid` must be asked again which rollout it holds,
/// `newest` being the newest rollout now. A different process, surely;
/// the same one, only when a rollout has started since the last look —
/// `/new` starts one in the same process. Any Codex starting one (a
/// teammate) costs one look, then counts as seen.
pub fn needs_lookup(known: Option<&KnownRollout>, pid: u32, newest: Option<&Path>) -> bool {
    let Some(known) = known else { return true };
    known.pid != pid || newest.and_then(Path::file_name) > known.newest.as_deref().and_then(Path::file_name)
}

/// The rollout the Codex at `pid` is writing. Codex keeps it open for as
/// long as the session runs, so the file is named by the process itself,
/// not guessed from a directory and a start time; the native binary that
/// holds it runs beneath the npm wrapper, hence the whole tree.
#[cfg(unix)]
pub fn open_rollout(pid: u32) -> Result<PathBuf, usize> {
    use crate::ports::{process_children, process_tree};
    let pids: Vec<String> = process_tree(pid, &process_children()).iter().map(u32::to_string).collect();
    let Ok(output) = crate::proc::command("lsof").args(["-nP", "-Fn", "-p", &pids.join(",")]).output() else {
        return Err(0);
    };
    let open: Vec<PathBuf> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix('n'))
        .map(PathBuf::from)
        .collect();
    rollout_of(&open)
}

/// Windows has no cheap way to list another process's open files.
#[cfg(not(unix))]
pub fn open_rollout(_pid: u32) -> Result<PathBuf, usize> {
    Err(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A rollout's lines in Codex 0.163's shapes, scrubbed: one turn, a
    /// command, a shell call, and two token counts (the first with no info).
    const ROLLOUT: &str = include_str!("../fixtures/codex-rollout.jsonl");

    fn stats<'a>(lines: impl Iterator<Item = &'a str>) -> TranscriptSummary {
        let mut stats = CodexStats::default();
        lines.for_each(|line| stats.feed(line));
        stats.summary()
    }

    #[test]
    fn a_rollout_is_named_after_its_thread() {
        let dir = std::env::temp_dir().join("sessions/2026/10/23");
        let id = "01a16910-50d2-7590-8de0-5e5e7b78b19c";
        assert_eq!(rollout_id(&dir.join(format!("rollout-2026-10-23T16-00-39-{id}.jsonl"))).as_deref(), Some(id));
        assert_eq!(rollout_id(&dir.join("rollout-2026-10-23T16-00-39.jsonl")), None);
        assert_eq!(rollout_id(&dir.join("rollout-2026-10-23T16-00-39-01a16910-50d2-7590-8de0-5e5e7b78b19z.jsonl")), None);
        assert_eq!(rollout_id(&dir.join(format!("notes-{id}.jsonl"))), None);
        assert_eq!(rollout_id(&dir.join("rollout-é.jsonl")), None);
    }

    #[test]
    fn a_codex_command_is_a_run_and_its_shell_call_too() {
        let mut trail = CodexTrail::default();
        ROLLOUT.lines().for_each(|l| trail.feed(l));
        let kinds: Vec<(&str, cmux_protocol::ActionKind)> =
            trail.turns()[0].actions.iter().map(|a| (a.tool.as_str(), a.kind)).collect();
        assert_eq!(
            kinds,
            [("CommandExecution", cmux_protocol::ActionKind::Run), ("shell", cmux_protocol::ActionKind::Run)]
        );
    }

    #[test]
    fn a_rollout_adds_up_to_codexs_own_counts() {
        let summary = stats(ROLLOUT.lines());
        assert_eq!(summary.model.as_deref(), Some("gpt-6-luna"));
        // Fresh: 37738 input less 23424 cached, plus 242 output.
        assert_eq!(summary.tokens, 37738 - 23424 + 242);
        // The last call's input, cached and not.
        assert_eq!(summary.context, 21087);
        assert_eq!(summary.context_window, Some(258_400));
        // The command and the shell call; not `exec`, the wrapper.
        assert_eq!(summary.tool_calls, 2);
        assert_eq!(summary.tasks, None);
        assert_eq!(summary.started_ms, parse_timestamp("2026-10-26T20:00:30.000Z"));
        assert_eq!(summary.last_ms, parse_timestamp("2026-10-26T20:00:40.000Z"));
    }

    #[test]
    fn a_rollout_with_no_token_count_yet_counts_nothing() {
        let summary = stats(ROLLOUT.lines().filter(|l| !l.contains("token_count")));
        assert_eq!(summary.model.as_deref(), Some("gpt-6-luna"));
        assert_eq!((summary.tokens, summary.context), (0, 0));
        // Not a Claude model, so the table has no window for it either.
        assert_eq!(summary.context_window, None);
    }

    fn trail(lines: &[String]) -> Vec<TrailTurn> {
        let mut trail = CodexTrail::default();
        for line in lines {
            trail.feed(line);
        }
        trail.turns
    }

    fn line(at: &str, kind: &str, payload: &str) -> String {
        format!(r#"{{"timestamp":"2026-10-25T10:00:{at}.000Z","type":"{kind}","payload":{payload}}}"#)
    }

    fn started(turn: &str, at: &str) -> String {
        line(at, "event_msg", &format!(r#"{{"type":"task_started","turn_id":"{turn}","started_at":1792922400}}"#))
    }

    fn complete(turn: &str, at: &str) -> String {
        line(at, "event_msg", &format!(r#"{{"type":"task_complete","turn_id":"{turn}","duration_ms":9000}}"#))
    }

    fn said(turn: &str, text: &str, at: &str) -> String {
        line(at, "event_msg", &format!(r#"{{"type":"item_completed","turn_id":"{turn}","item":{{"type":"UserMessage","id":"u-{turn}","content":[{{"type":"text","text":"{text}"}}]}}}}"#))
    }

    fn item(turn: &str, item: &str, from_ms: u64, to_ms: u64) -> String {
        line("05", "event_msg", &format!(r#"{{"type":"item_completed","turn_id":"{turn}","item":{item},"started_at_ms":{from_ms},"completed_at_ms":{to_ms}}}"#))
    }

    fn command(id: &str, script: &str, exit: i32, output: &str) -> String {
        let status = if exit == 0 { "completed" } else { "failed" };
        format!(r#"{{"type":"CommandExecution","id":"{id}","command":["/bin/zsh","-lc","{script}"],"cwd":"/w","status":"{status}","aggregated_output":"{output}","exit_code":{exit},"duration":{{"secs":1,"nanos":0}}}}"#)
    }

    #[test]
    fn reads_turns_and_the_items_done_in_them() {
        let t0 = 1_792_922_400_000;
        let turns = trail(&[
            line("00", "session_meta", r#"{"id":"s","cwd":"/w","cli_version":"0.163.0"}"#),
            started("A", "00"),
            said("A", r"Fix the build\nthen test", "00"),
            // The script its tools run in: a wrapper, not an action.
            line("01", "response_item", r#"{"type":"custom_tool_call","call_id":"x1","name":"exec","input":"const r = 1;","internal_chat_message_metadata_passthrough":{"turn_id":"A"}}"#),
            item("A", &command("c1", "cargo build", 101, r"error[E0425]\nmore"), t0 + 1000, t0 + 4000),
            item("A", r#"{"type":"FileChange","id":"f1","changes":{"/w/src/a.rs":{"type":"update","unified_diff":"@@"},"/w/src/b.rs":{"type":"add","content":"x"}},"status":"completed"}"#, t0 + 5000, t0 + 5100),
            item("A", r#"{"type":"Reasoning","id":"r","summary_text":[]}"#, t0, t0),
            item("A", r#"{"type":"AgentMessage","id":"m","content":[]}"#, t0, t0),
            complete("A", "09"),
            started("B", "10"),
            said("B", "Now test", "10"),
            item("B", &command("c2", "cargo test", 0, "ok"), t0 + 11_000, t0 + 12_000),
            item("B", r#"{"type":"ImageView","id":"i1","path":"/w/shot.png"}"#, t0 + 12_000, t0 + 12_000),
            item("B", r#"{"type":"Extension","id":"e1","kind":"web_search","query":"rust lsof","results":[]}"#, t0 + 12_000, t0 + 13_000),
        ]);
        assert_eq!(turns.len(), 2);
        let a = &turns[0];
        assert_eq!(a.prompt.as_deref(), Some("Fix the build…"));
        assert_eq!(a.text.as_deref(), Some("Fix the build\nthen test"));
        assert_eq!((a.started_ms, a.last_ms), (Some(t0), Some(t0 + 9000)));
        let summed: Vec<(&str, &str, ActionStatus, Option<i32>)> = a
            .actions
            .iter()
            .map(|x| (x.tool.as_str(), x.summary.as_str(), x.status, x.exit_code))
            .collect();
        assert_eq!(
            summed,
            [
                ("CommandExecution", "cargo build", ActionStatus::Error, Some(101)),
                ("FileChange", "/w/src/a.rs, /w/src/b.rs", ActionStatus::Ok, None),
            ]
        );
        let durations: Vec<Option<u64>> = a.actions.iter().map(|x| x.duration_ms).collect();
        assert_eq!(durations, [Some(3000), Some(100)]);
        assert_eq!(a.actions[0].output, "error[E0425]\nmore");
        let b: Vec<(&str, &str)> = turns[1].actions.iter().map(|x| (x.tool.as_str(), x.summary.as_str())).collect();
        assert_eq!(b, [("CommandExecution", "cargo test"), ("ImageView", "/w/shot.png"), ("Extension", "rust lsof")]);
        let kinds: Vec<cmux_protocol::ActionKind> = turns[1].actions.iter().map(|x| x.kind).collect();
        use cmux_protocol::ActionKind::{Read, Run};
        assert_eq!(kinds, [Run, Read, Read], "an image viewed and a web search are reads");
        assert_eq!(turns[1].prompt.as_deref(), Some("Now test"));
    }

    #[test]
    fn pairs_calls_with_their_output_by_call_id() {
        let patch = r"*** Begin Patch\n*** Update File: src/a.rs\n@@\n-a\n+b\n*** Add File: src/c.rs\n+c\n*** End Patch";
        let turns = trail(&[
            started("A", "00"),
            line("00", "event_msg", r#"{"type":"user_message","message":"Patch it"}"#),
            line("01", "response_item", r#"{"type":"function_call","call_id":"s1","name":"shell","arguments":"{\"command\":[\"bash\",\"-lc\",\"make\"]}"}"#),
            line("02", "response_item", &format!(r#"{{"type":"custom_tool_call","call_id":"p1","name":"apply_patch","input":"{patch}"}}"#)),
            line("03", "response_item", r#"{"type":"function_call","call_id":"e1","name":"exec_command","arguments":"{\"cmd\":\"ls -la\"}"}"#),
            line("04", "response_item", r#"{"type":"function_call","call_id":"w1","name":"wait","arguments":"{\"cell_id\":\"1\"}"}"#),
            line("05", "response_item", r#"{"type":"function_call","call_id":"q1","name":"view_image","arguments":"{\"path\":\"/w/a.png\"}"}"#),
            line("06", "response_item", r#"{"type":"function_call_output","call_id":"s1","output":"{\"output\":\"make: *** Error 2\",\"metadata\":{\"exit_code\":2,\"duration_seconds\":0.5}}"}"#),
            line("07", "response_item", r#"{"type":"custom_tool_call_output","call_id":"p1","output":[{"type":"input_text","text":"Success. Updated the following files"}]}"#),
            line("08", "response_item", r#"{"type":"function_call_output","call_id":"e1","output":"Process exited with code 0\nOutput:\ntotal 0"}"#),
            line("09", "response_item", r#"{"type":"function_call_output","call_id":"nobody","output":"orphan"}"#),
        ]);
        let summed: Vec<(&str, &str, ActionStatus, Option<i32>)> = turns[0]
            .actions
            .iter()
            .map(|x| (x.tool.as_str(), x.summary.as_str(), x.status, x.exit_code))
            .collect();
        assert_eq!(
            summed,
            [
                ("shell", "make", ActionStatus::Error, Some(2)),
                ("apply_patch", "src/a.rs, src/c.rs", ActionStatus::Ok, None),
                ("exec_command", "ls -la", ActionStatus::Ok, Some(0)),
                ("view_image", "view_image: /w/a.png", ActionStatus::Running, None),
            ]
        );
        assert_eq!(turns[0].prompt.as_deref(), Some("Patch it"));
        use cmux_protocol::ActionKind::{Edit, Read, Run};
        let kinds: Vec<cmux_protocol::ActionKind> = turns[0].actions.iter().map(|x| x.kind).collect();
        assert_eq!(kinds, [Run, Edit, Run, Read], "view_image is a read");
        assert_eq!(turns[0].actions[0].output, "make: *** Error 2");
        assert_eq!(turns[0].actions[0].duration_ms, Some(5000));
    }

    #[test]
    fn a_turn_that_ends_leaves_no_call_running() {
        let turns = trail(&[
            started("A", "00"),
            line("01", "response_item", r#"{"type":"function_call","call_id":"s1","name":"shell","arguments":"{\"command\":\"sleep 99\"}","internal_chat_message_metadata_passthrough":{"turn_id":"A"}}"#),
            line("02", "event_msg", r#"{"type":"turn_aborted","turn_id":"A","reason":"interrupted"}"#),
            started("B", "03"),
            line("04", "response_item", r#"{"type":"function_call","call_id":"s2","name":"shell","arguments":"{\"command\":\"make\"}","internal_chat_message_metadata_passthrough":{"turn_id":"B"}}"#),
        ]);
        assert_eq!(turns[0].actions[0].status, ActionStatus::Interrupted);
        assert_eq!(turns[0].last_ms, Some(1_792_922_402_000));
        assert_eq!(turns[1].actions[0].status, ActionStatus::Running);
    }

    #[test]
    fn skips_what_does_not_parse() {
        let turns = trail(&[
            String::new(),
            "{".into(),
            "null".into(),
            r#"{"type":"event_msg"}"#.into(),
            r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"type":"CommandExecution"}}}"#.into(),
            r#"{"type":"response_item","payload":{"type":"function_call","name":"shell"}}"#.into(),
            r#"{"type":"event_msg","payload":{"type":"task_complete","turn_id":"none"}}"#.into(),
        ]);
        assert!(turns.is_empty());
    }

    #[test]
    fn reads_one_action_in_full_from_the_rollout() {
        let dir = std::env::temp_dir().join(format!("mira-codex-trail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rollout-1.jsonl");
        let many: String = (1..=40).map(|n| format!("{n}\\n")).collect();
        let lines = [
            started("A", "00"),
            item("A", &command("c1", "seq 40", 0, &many), 0, 10),
            line("02", "response_item", r#"{"type":"function_call","call_id":"s1","name":"shell","arguments":"{\"command\":\"false\"}"}"#),
            line("03", "response_item", r#"{"type":"function_call_output","call_id":"s1","output":"Process exited with code 1\nOutput:\n"}"#),
            line("04", "response_item", r#"{"type":"function_call","call_id":"s2","name":"shell","arguments":"{\"command\":\"sleep 9\"}"}"#),
        ];
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();

        let c1 = action_detail(&path, "c1").unwrap();
        assert_eq!((c1.tool.as_str(), c1.is_error), ("CommandExecution", false));
        assert_eq!(c1.output.as_deref().map(|o| o.lines().count()), Some(40));
        assert!(c1.input.contains("seq 40") && !c1.input.contains("aggregated_output"));
        let s1 = action_detail(&path, "s1").unwrap();
        assert_eq!((s1.input.as_str(), s1.is_error), ("{\n  \"command\": \"false\"\n}", true));
        assert_eq!(action_detail(&path, "s2").unwrap().output, None);
        assert_eq!(action_detail(&path, "zzz"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_item_with_an_error_failed_in_the_trail_and_its_detail() {
        let dir = std::env::temp_dir().join(format!("mira-codex-error-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rollout-1.jsonl");
        // Completed, no exit code, but an error.
        let mcp = r#"{"type":"McpToolCall","id":"m1","server":"docs","tool":"read","status":"completed","error":{"message":"not found"}}"#;
        let lines = [started("A", "00"), item("A", mcp, 0, 10)];
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();

        let turns = trail(&lines);
        assert_eq!(turns[0].actions[0].status, ActionStatus::Error);
        assert!(action_detail(&path, "m1").unwrap().is_error);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn looks_again_only_when_a_rollout_may_have_changed() {
        let day = std::env::temp_dir().join(".codex/sessions/2026/10/25");
        let mine = day.join("rollout-2026-10-25T10-00-00-a.jsonl");
        let later = day.join("rollout-2026-10-25T11-00-00-b.jsonl");
        let known = KnownRollout { pid: 7, path: mine.clone(), newest: Some(mine.clone()) };
        // Never looked, or another process: look.
        assert!(needs_lookup(None, 7, Some(&mine)));
        assert!(needs_lookup(Some(&known), 8, Some(&mine)));
        // The same process, nothing newer: what was found holds.
        assert!(!needs_lookup(Some(&known), 7, Some(&mine)));
        assert!(!needs_lookup(Some(&known), 7, None));
        // The same process, a rollout started since (its `/new`, or a
        // teammate's): look, once.
        assert!(needs_lookup(Some(&known), 7, Some(&later)));
        let seen = KnownRollout { newest: Some(later.clone()), ..known };
        assert!(!needs_lookup(Some(&seen), 7, Some(&later)));
    }

    #[test]
    fn finds_the_newest_rollout_by_its_day_and_name() {
        let sessions = std::env::temp_dir().join(format!("mira-newest-{}/sessions", std::process::id()));
        let _ = std::fs::remove_dir_all(&sessions);
        let touch = |rel: &str| {
            let p = sessions.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "").unwrap();
            p
        };
        assert_eq!(newest_rollout(&sessions), None);
        touch("2026/09/30/rollout-2026-09-30T23-00-00-a.jsonl");
        touch("2026/10/24/rollout-2026-10-24T09-00-00-b.jsonl");
        let newest = touch("2026/10/24/rollout-2026-10-24T18-00-00-c.jsonl");
        touch("2026/10/24/notes.txt");
        assert_eq!(newest_rollout(&sessions), Some(newest.clone()));
        assert_eq!(sessions_of(&newest), Some(sessions.as_path()));
        let _ = std::fs::remove_dir_all(sessions.parent().unwrap());
    }

    #[test]
    fn tells_each_turn_end_once_from_where_it_began_following() {
        let dir = std::env::temp_dir().join(format!("mira-tail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rollout-1.jsonl");
        let append = |lines: &[String]| {
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path).unwrap();
            std::io::Write::write_all(&mut f, (lines.join("\n") + "\n").as_bytes()).unwrap();
        };
        // A session's history, there before Mirador looked: not replayed.
        append(&[started("A", "00"), complete("A", "01")]);
        let mut tail = RolloutTail::at_end(path.clone());
        assert_eq!(tail.read(), []);

        // A turn ending now is told once, however often it is read.
        append(&[started("B", "02"), item("B", &command("c", "ls", 0, "x"), 0, 1), complete("B", "03")]);
        assert_eq!(tail.read(), [TurnEnd::Done]);
        assert_eq!(tail.read(), []);

        // Interrupted is an end too; one turn's second end is not.
        append(&[started("C", "04"), line("05", "event_msg", r#"{"type":"turn_aborted","turn_id":"C"}"#)]);
        append(&[complete("C", "06")]);
        assert_eq!(tail.read(), [TurnEnd::Done]);

        // A line still being written waits for the rest of it.
        let done = complete("D", "07");
        let (head, rest) = done.split_at(20);
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        std::io::Write::write_all(&mut f, head.as_bytes()).unwrap();
        assert_eq!(tail.read(), []);
        std::io::Write::write_all(&mut f, format!("{rest}\n").as_bytes()).unwrap();
        assert_eq!(tail.read(), [TurnEnd::Done]);

        // Rewritten shorter: followed from its new end, nothing replayed.
        std::fs::write(&path, format!("{}\n", complete("E", "08"))).unwrap();
        assert_eq!(tail.read(), []);
        append(&[complete("F", "09")]);
        assert_eq!(tail.read(), [TurnEnd::Done]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn follows_a_pane_s_new_session_from_its_start() {
        let day = std::env::temp_dir().join(".codex/sessions/2026/10/25");
        let old = day.join("rollout-2026-10-25T10-00-00-a.jsonl");
        let new = day.join("rollout-2026-10-25T11-00-00-b.jsonl");
        // First seen: its history is not logged.
        assert!(!follow_from_start(None, &old));
        // A newer one after it (`/new`): all of it is new work.
        assert!(follow_from_start(Some(&old), &new));
        // An older one reopened (`codex resume`), or the same: history.
        assert!(!follow_from_start(Some(&new), &old));
        assert!(!follow_from_start(Some(&old), &old));
    }

    #[test]
    fn names_the_rollout_only_when_there_is_one() {
        let home = std::env::temp_dir().join(".codex");
        let rollout = home.join("sessions/2026/10/25/rollout-2026-10-25T10-00-00-abc.jsonl");
        let other = home.join("sessions/2026/10/25/rollout-2026-10-25T10-00-01-def.jsonl");
        let noise = [home.join("log/codex-tui.log"), home.join("rollout-stray.jsonl"), PathBuf::from("/dev/ttys002")];
        let open = |paths: &[&PathBuf]| -> Vec<PathBuf> { noise.iter().chain(paths.iter().copied()).cloned().collect() };

        assert_eq!(rollout_of(&open(&[&rollout])), Ok(rollout.clone()));
        // The same file on two descriptors is still one.
        assert_eq!(rollout_of(&open(&[&rollout, &rollout])), Ok(rollout.clone()));
        // Two sessions held open (a Codex it started, or a thread per
        // agent): which is this pane's is not known, so none is shown.
        assert_eq!(rollout_of(&open(&[&rollout, &other])), Err(2));
        assert_eq!(rollout_of(&open(&[])), Err(0));
    }
}
