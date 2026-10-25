//! What a Claude Code session and its subagents are doing, read from the
//! transcripts Claude Code writes as it goes.
//!
//! A session's transcript is `…/projects/<project>/<session>.jsonl`, and
//! each subagent it starts (the `Agent` tool) gets
//! `…/<session>/subagents/agent-<id>.jsonl`, with a `.meta.json` beside it
//! holding the `Agent` call's description and type. Every line is one JSON
//! entry. Nothing here is a documented format, so every field is optional
//! and an entry that does not parse is skipped rather than failing the read.
//!
//! A session transcript grows to tens of megabytes, and the panel showing
//! this polls every two seconds, so [`Cache`] reads each file once and then
//! only what was appended since. It adds up [`Stats`] here, and the trail of
//! what an agent did in `trail`.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use cmux_protocol::{SubagentInfo, SubagentState, TaskProgress, TranscriptSummary};
use serde_json::Value;

/// One task of a `TaskCreate` list.
#[derive(Debug, Clone, Default)]
struct Task {
    status: String,
    subject: String,
    active_form: Option<String>,
}

/// One transcript, added up entry by entry.
#[derive(Debug, Default)]
pub struct Stats {
    model: Option<String>,
    tokens: u64,
    context: u64,
    tool_calls: u32,
    /// The reply being counted, and what it added to `tokens`: Claude Code
    /// writes one reply as several entries (thinking, text, each tool
    /// call), each repeating the reply's usage, so a repeat replaces
    /// rather than adds.
    reply: Option<(String, u64)>,
    running: bool,
    /// By task id, in id order.
    tasks: BTreeMap<String, Task>,
    /// `TaskCreate` calls awaiting the result that names their task id.
    creating: HashMap<String, Task>,
    /// The last `TodoWrite` list, for versions before `TaskCreate`.
    todos: Option<Vec<Task>>,
    started_ms: Option<u64>,
    last_ms: Option<u64>,
}

/// What a [`Cache`] adds a transcript up into, one line at a time.
pub trait Feed: Default {
    fn feed(&mut self, line: &str);
}

impl Feed for Stats {
    fn feed(&mut self, line: &str) {
        Stats::feed(self, line)
    }
}

impl Stats {
    /// Adds one line of a transcript.
    pub fn feed(&mut self, line: &str) {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            return;
        };
        if let Some(ms) = entry["timestamp"].as_str().and_then(parse_timestamp) {
            self.started_ms.get_or_insert(ms);
            self.last_ms = Some(ms);
        }
        // Lines Claude Code adds for itself (caveats, injected context)
        // are not the conversation.
        if entry["isMeta"].as_bool() == Some(true) {
            return;
        }
        match entry["type"].as_str() {
            Some("assistant") => self.assistant(&entry["message"]),
            Some("user") => self.user(&entry),
            _ => {}
        }
    }

    fn assistant(&mut self, message: &Value) {
        if !message.is_object() {
            return;
        }
        if let Some(model) = message["model"].as_str().filter(|m| !m.starts_with('<')) {
            self.model = Some(model.to_string());
        }
        let usage = &message["usage"];
        if usage.is_object() {
            let n = |key: &str| usage[key].as_u64().unwrap_or(0);
            let fresh = n("input_tokens") + n("cache_creation_input_tokens") + n("output_tokens");
            let id = message["id"].as_str().unwrap_or("").to_string();
            match &mut self.reply {
                Some((current, counted)) if !id.is_empty() && *current == id => {
                    self.tokens = self.tokens - *counted + fresh;
                    *counted = fresh;
                }
                _ => {
                    self.tokens += fresh;
                    self.reply = Some((id, fresh));
                }
            }
            self.context =
                n("input_tokens") + n("cache_creation_input_tokens") + n("cache_read_input_tokens");
        }
        let blocks = message["content"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        let mut calls = 0;
        let mut handed_back = false;
        for block in blocks.iter().filter(|b| b["type"] == "tool_use") {
            calls += 1;
            let input = &block["input"];
            match block["name"].as_str() {
                Some("SubagentHandback") => handed_back = true,
                Some("TaskCreate") => {
                    if let Some(id) = block["id"].as_str() {
                        self.creating.insert(
                            id.to_string(),
                            Task {
                                status: "pending".into(),
                                subject: text(&input["subject"]).unwrap_or_default(),
                                active_form: text(&input["activeForm"]),
                            },
                        );
                    }
                }
                Some("TaskUpdate") => self.update_task(input),
                Some("TodoWrite") => {
                    self.todos = input["todos"].as_array().map(|todos| {
                        todos
                            .iter()
                            .map(|t| Task {
                                status: text(&t["status"]).unwrap_or_default(),
                                subject: text(&t["content"]).unwrap_or_default(),
                                active_form: text(&t["activeForm"]),
                            })
                            .collect()
                    });
                }
                _ => {}
            }
        }
        self.tool_calls += calls;
        // Handing back the report ends a background subagent's work; a
        // tool call means more to come; a reply with a stop reason ends
        // the turn. A thinking or text entry without one is the start of
        // a reply still being written.
        self.running = if handed_back {
            false
        } else if calls > 0 {
            true
        } else {
            message["stop_reason"].as_str().is_none()
        };
    }

    fn update_task(&mut self, input: &Value) {
        let Some(id) = text(&input["taskId"]) else { return };
        if text(&input["status"]).as_deref() == Some("deleted") {
            self.tasks.remove(&id);
            return;
        }
        // A task this transcript did not create (a list shared with other
        // sessions) is not counted.
        let Some(task) = self.tasks.get_mut(&id) else { return };
        if let Some(status) = text(&input["status"]) {
            task.status = status;
        }
        if let Some(subject) = text(&input["subject"]) {
            task.subject = subject;
        }
        if let Some(active) = text(&input["activeForm"]) {
            task.active_form = Some(active);
        }
    }

    fn user(&mut self, entry: &Value) {
        let content = &entry["message"]["content"];
        // The result of a `TaskCreate` names the task's id.
        if let Some(id) = text(&entry["toolUseResult"]["task"]["id"]) {
            let created = content.as_array().into_iter().flatten().find_map(|b| {
                self.creating.remove(b["tool_use_id"].as_str()?)
            });
            if let Some(task) = created {
                self.tasks.insert(id, task);
            }
        }
        // A prompt (a first one, or a message resuming a subagent) starts
        // work; a tool's result leaves it as the call made it.
        let prompt = match content {
            Value::String(s) => Some(s.as_str()),
            Value::Array(blocks) => blocks
                .iter()
                .find(|b| b["type"] == "text")
                .and_then(|b| b["text"].as_str()),
            _ => None,
        };
        if let Some(prompt) = prompt {
            self.running = !prompt.starts_with("[Request interrupted");
        }
    }

    /// Whether its last entry leaves it working.
    pub fn running(&self) -> bool {
        self.running
    }

    pub fn summary(&self) -> TranscriptSummary {
        TranscriptSummary {
            model: self.model.clone(),
            tokens: self.tokens,
            context: self.context,
            tool_calls: self.tool_calls,
            tasks: self.progress(),
            started_ms: self.started_ms,
            last_ms: self.last_ms,
        }
    }

    fn progress(&self) -> Option<TaskProgress> {
        let tasks: Vec<&Task> = if self.tasks.is_empty() {
            self.todos.iter().flatten().collect()
        } else {
            self.tasks.values().collect()
        };
        if tasks.is_empty() {
            return None;
        }
        let current = tasks
            .iter()
            .find(|t| t.status == "in_progress")
            .map(|t| t.active_form.clone().unwrap_or_else(|| t.subject.clone()));
        Some(TaskProgress {
            done: tasks.iter().filter(|t| t.status == "completed").count() as u32,
            total: tasks.len() as u32,
            current,
        })
    }
}

pub(crate) fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Unix millis of an RFC 3339 UTC timestamp as Claude Code writes them
/// (`2026-10-04T08:49:29.163Z`).
pub(crate) fn parse_timestamp(s: &str) -> Option<u64> {
    let s = s.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let (hms, frac) = time.split_once('.').unwrap_or((time, "0"));
    let mut t = hms.splitn(3, ':').map(|p| p.parse::<i64>().ok());
    let (hh, mm, ss) = (t.next()??, t.next()??, t.next()??);
    let millis: i64 = format!("{frac:0<3}").get(..3)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let ms = ((days * 24 + hh) * 60 + mm) * 60_000 + ss * 1000 + millis;
    u64::try_from(ms).ok()
}

/// Where a session's subagents' transcripts are.
pub fn subagent_dir(transcript: &Path) -> PathBuf {
    transcript.with_extension("").join("subagents")
}

/// Whether `path` can be a transcript of `session_id`: the hook says where
/// it is, and nothing else is ever read on its word.
pub fn is_transcript_of(path: &Path, session_id: &str) -> bool {
    path.is_absolute()
        && path.extension().is_some_and(|e| e == "jsonl")
        && path.file_stem().is_some_and(|s| s == session_id)
}

/// Every transcript read so far, and how far.
pub struct Cache<T = Stats> {
    files: HashMap<PathBuf, (u64, T)>,
}

impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self { files: HashMap::new() }
    }
}

impl<T: Feed> Cache<T> {
    /// The transcript at `path`, brought up to date with what was appended
    /// since the last read. A file that shrank was rewritten, and is read
    /// again from the start.
    pub fn read(&mut self, path: &Path) -> Option<&T> {
        // Bounded: a session with hundreds of subagents is read again on
        // the next poll rather than kept forever.
        if self.files.len() > 512 && !self.files.contains_key(path) {
            self.files.clear();
        }
        let mut file = std::fs::File::open(path).ok()?;
        let len = file.metadata().ok()?.len();
        let (offset, stats) = self.files.entry(path.to_path_buf()).or_default();
        if len < *offset {
            *offset = 0;
            *stats = T::default();
        }
        if len > *offset {
            file.seek(SeekFrom::Start(*offset)).ok()?;
            let mut bytes = Vec::with_capacity((len - *offset) as usize);
            file.take(len - *offset).read_to_end(&mut bytes).ok()?;
            // A line still being written waits for the next read.
            let complete = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
            for line in bytes[..complete].split(|&b| b == b'\n') {
                if !line.is_empty() {
                    stats.feed(&String::from_utf8_lossy(line));
                }
            }
            *offset += complete as u64;
        }
        Some(stats)
    }
}

impl Cache {
    /// A session and its subagents. `live`: the session's Claude Code is
    /// running, without which a subagent it hosted cannot be.
    pub fn activity(&mut self, transcript: &Path, live: bool) -> (TranscriptSummary, Vec<SubagentInfo>) {
        let session = self.read(transcript).map(Stats::summary).unwrap_or_else(|| Stats::default().summary());
        let dir = subagent_dir(transcript);
        let mut ids: Vec<String> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let id = name.strip_prefix("agent-")?.strip_suffix(".jsonl")?;
                Some(id.to_string())
            })
            .collect();
        ids.sort();
        let mut subagents: Vec<SubagentInfo> = ids
            .into_iter()
            .filter_map(|id| {
                let stats = self.read(&dir.join(format!("agent-{id}.jsonl")))?;
                let state = match (stats.running(), live) {
                    (false, _) => SubagentState::Done,
                    (true, true) => SubagentState::Running,
                    (true, false) => SubagentState::Stopped,
                };
                let summary = stats.summary();
                let meta: Value = std::fs::read(dir.join(format!("agent-{id}.meta.json")))
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or(Value::Null);
                Some(SubagentInfo {
                    description: text(&meta["description"]),
                    agent_type: text(&meta["agentType"]),
                    depth: meta["spawnDepth"].as_u64().unwrap_or(1) as u32,
                    state,
                    summary,
                    id,
                })
            })
            .collect();
        subagents.sort_by_key(|s| {
            (
                s.state != SubagentState::Running,
                std::cmp::Reverse(s.summary.last_ms),
            )
        });
        (session, subagents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(lines: &[&str]) -> Stats {
        let mut stats = Stats::default();
        for line in lines {
            stats.feed(line);
        }
        stats
    }

    const PROMPT: &str = r#"{"type":"user","timestamp":"2026-10-04T08:49:29.163Z","message":{"role":"user","content":"Review the kit"}}"#;

    #[test]
    fn counts_each_reply_once_however_many_entries_carry_it() {
        // One reply in three entries, each repeating its usage.
        let usage = r#""usage":{"input_tokens":2,"cache_creation_input_tokens":1000,"cache_read_input_tokens":12000,"output_tokens":50}"#;
        let stats = feed(&[
            PROMPT,
            &format!(r#"{{"type":"assistant","message":{{"id":"m1","model":"claude-opus-5-5","content":[{{"type":"thinking"}}],{usage}}}}}"#),
            &format!(r#"{{"type":"assistant","message":{{"id":"m1","model":"claude-opus-5-5","content":[{{"type":"text","text":"ok"}}],{usage}}}}}"#),
            &format!(r#"{{"type":"assistant","timestamp":"2026-10-04T08:50:29.163Z","message":{{"id":"m1","model":"claude-opus-5-5","content":[{{"type":"tool_use","id":"t1","name":"Read","input":{{}}}}],{usage}}}}}"#),
        ]);
        let s = stats.summary();
        assert_eq!(s.tokens, 1052);
        assert_eq!(s.context, 13002);
        assert_eq!(s.tool_calls, 1);
        assert_eq!(s.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(s.started_ms, Some(1_791_103_769_163));
        assert_eq!(s.last_ms, Some(1_791_103_829_163));
        assert!(stats.running());
    }

    #[test]
    fn tracks_the_task_list() {
        let create = |n: u32, subject: &str| {
            [
                format!(r#"{{"type":"assistant","message":{{"id":"c{n}","content":[{{"type":"tool_use","id":"tc{n}","name":"TaskCreate","input":{{"subject":"{subject}","activeForm":"{subject}ing"}}}}]}}}}"#),
                format!(r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"tc{n}","content":"Task #{n} created"}}]}},"toolUseResult":{{"task":{{"id":"{n}","subject":"{subject}"}}}}}}"#),
            ]
        };
        let update = |id: &str, status: &str| {
            format!(r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"u","name":"TaskUpdate","input":{{"taskId":"{id}","status":"{status}"}}}}]}}}}"#)
        };
        let mut lines: Vec<String> = vec![PROMPT.to_string()];
        for (n, s) in [(1, "Read"), (2, "Fix"), (3, "Test"), (4, "Drop")] {
            lines.extend(create(n, s));
        }
        lines.push(update("1", "completed"));
        lines.push(update("2", "in_progress"));
        lines.push(update("4", "deleted"));
        // Another session's task is not ours to count.
        lines.push(update("99", "completed"));
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let tasks = feed(&refs).summary().tasks.unwrap();
        assert_eq!((tasks.done, tasks.total), (1, 3));
        assert_eq!(tasks.current.as_deref(), Some("Fixing"));
    }

    #[test]
    fn falls_back_to_the_last_todo_list() {
        let stats = feed(&[
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t","name":"TodoWrite","input":{"todos":[{"content":"a","status":"completed"},{"content":"b","status":"pending"}]}}]}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t","name":"TodoWrite","input":{"todos":[{"content":"a","status":"completed"},{"content":"b","status":"in_progress","activeForm":"Doing b"}]}}]}}"#,
        ]);
        let tasks = stats.summary().tasks.unwrap();
        assert_eq!((tasks.done, tasks.total), (1, 2));
        assert_eq!(tasks.current.as_deref(), Some("Doing b"));
        assert_eq!(feed(&[PROMPT]).summary().tasks, None);
    }

    #[test]
    fn knows_when_a_subagent_is_done() {
        let end_turn = r#"{"type":"assistant","message":{"id":"m2","content":[{"type":"text","text":"Report"}],"stop_reason":"end_turn"}}"#;
        let tool = r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t","name":"Bash","input":{}}]}}"#;
        let result = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t","content":"ok"}]}}"#;
        let handback = r#"{"type":"assistant","message":{"id":"m3","content":[{"type":"tool_use","id":"h","name":"SubagentHandback","input":{"message":"done"}}]}}"#;
        let handback_result = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"h","content":"ok"}]}}"#;
        let resumed = r#"{"type":"user","message":{"content":[{"type":"text","text":"One more thing"}]}}"#;
        let meta = r#"{"type":"user","isMeta":true,"message":{"content":"<caveat>"}}"#;

        assert!(feed(&[PROMPT]).running());
        assert!(feed(&[PROMPT, tool, result]).running());
        assert!(!feed(&[PROMPT, tool, result, end_turn]).running());
        assert!(!feed(&[PROMPT, end_turn, meta]).running());
        // A background subagent hands its report back, and may be resumed.
        assert!(!feed(&[PROMPT, tool, result, handback, handback_result]).running());
        assert!(feed(&[PROMPT, handback, handback_result, resumed]).running());
        assert!(!feed(&[PROMPT, tool, r#"{"type":"user","message":{"content":"[Request interrupted by user]"}}"#]).running());
    }

    #[test]
    fn skips_what_does_not_parse() {
        let stats = feed(&["", "{", "null", r#"{"type":"assistant","message":"x"}"#, r#"{"type":"user"}"#]);
        assert_eq!(stats.summary().tokens, 0);
        assert!(!stats.running());
    }

    #[test]
    fn parses_timestamps() {
        assert_eq!(parse_timestamp("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(parse_timestamp("2000-03-01T00:00:01Z"), Some(951_868_801_000));
        assert_eq!(parse_timestamp("2026-10-04T08:49:29.5Z"), Some(1_791_103_769_500));
        assert_eq!(parse_timestamp("2026-13-04T08:49:29Z"), None);
        assert_eq!(parse_timestamp("yesterday"), None);
    }

    #[test]
    fn accepts_only_the_session_s_own_transcript() {
        // Absolute on every platform: `/u/…` has no drive, so on Windows
        // it is relative, and rightly refused.
        let root = std::env::temp_dir();
        let p = root.join(".claude/projects/-u-w/abc-1.jsonl");
        assert!(is_transcript_of(&p, "abc-1"));
        assert!(!is_transcript_of(&p, "abc-2"));
        assert!(!is_transcript_of(&root.join("passwd"), "passwd"));
        assert!(!is_transcript_of(Path::new("abc-1.jsonl"), "abc-1"));
        assert_eq!(subagent_dir(&p), root.join(".claude/projects/-u-w/abc-1/subagents"));
    }

    #[test]
    fn reads_a_session_and_its_subagents_incrementally() {
        let root = std::env::temp_dir().join(format!("mira-transcript-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let session = root.join("s1.jsonl");
        let subs = subagent_dir(&session);
        std::fs::create_dir_all(&subs).unwrap();
        std::fs::write(&session, format!("{PROMPT}\n")).unwrap();
        std::fs::write(
            subs.join("agent-a1.meta.json"),
            r#"{"agentType":"Explore","description":"Map the crates","spawnDepth":1}"#,
        )
        .unwrap();
        let tool = r#"{"type":"assistant","timestamp":"2026-10-04T08:50:00.000Z","message":{"id":"m","content":[{"type":"tool_use","id":"t","name":"Read","input":{}}]}}"#;
        // The last line is still being written.
        std::fs::write(subs.join("agent-a1.jsonl"), format!("{PROMPT}\n{tool}\n{{\"type\":\"assi")).unwrap();
        std::fs::write(subs.join("agent-a2.jsonl"), format!("{PROMPT}\n{}\n", r#"{"type":"assistant","message":{"content":[{"type":"text","text":"x"}],"stop_reason":"end_turn"}}"#)).unwrap();

        let mut cache: Cache = Cache::default();
        let (_, agents) = cache.activity(&session, true);
        let states: Vec<(&str, SubagentState)> = agents.iter().map(|a| (a.id.as_str(), a.state)).collect();
        assert_eq!(states, [("a1", SubagentState::Running), ("a2", SubagentState::Done)]);
        assert_eq!(agents[0].description.as_deref(), Some("Map the crates"));
        assert_eq!(agents[0].agent_type.as_deref(), Some("Explore"));
        assert_eq!(agents[1].description, None);

        // Its session gone, a running subagent is stopped.
        let (_, agents) = cache.activity(&session, false);
        assert_eq!(agents[0].state, SubagentState::Stopped);

        // The half-written line, finished, is read on the next poll.
        let mut f = std::fs::OpenOptions::new().append(true).open(subs.join("agent-a1.jsonl")).unwrap();
        std::io::Write::write_all(&mut f, br#"stant","message":{"id":"m9","content":[{"type":"text","text":"done"}],"stop_reason":"end_turn"}}
"#).unwrap();
        let (_, agents) = cache.activity(&session, true);
        let a1 = agents.iter().find(|a| a.id == "a1").unwrap();
        assert_eq!(a1.state, SubagentState::Done);
        assert_eq!(a1.summary.tool_calls, 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
