//! The task log: every task given to an agent, every card moved, every
//! turn ended, one JSON line each in `tasks.jsonl`, kept across restarts.
//! The board is not stored: [`cards`] folds the log into it on each read.
//!
//! Cards come from two sides. Mirador logs a task each time it gives one —
//! starting a team or an agent, `mira agent send` — and that card follows
//! its agent: Doing when given, Needs you or Done as its turns end, and
//! Done once the agent is given a newer task. A card written with
//! `mira task add` moves only when someone moves it.
//!
//! The log is only appended to while the app runs, so a crash can leave a
//! last line half-written; reading skips any line that does not parse, and
//! the next append starts on a fresh line. [`compact`] rewrites it at most
//! [`KEEP_EVENTS`] long and [`KEEP_DAYS`] deep.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use cmux_protocol::{AgentStatus, TaskCard, TaskColumn, TaskEvent, TaskEventKind, TaskHistoryEntry, TaskSource, TurnEnd};

use crate::session::{atomic_write, data_dir};

/// How far back the log goes once compacted...
pub const KEEP_DAYS: u64 = 30;
/// ...and how many events it keeps at most, the newest.
pub const KEEP_EVENTS: usize = 10_000;

pub fn log_path() -> PathBuf {
    data_dir().join("tasks.jsonl")
}

/// A new card id: short enough to type after `mira task move`.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

/// Adds `event` to the end of the log at `path`.
pub fn append(path: &Path, event: &TaskEvent) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(path)?;
    let mut line = serde_json::to_string(event)?;
    line.push('\n');
    // A line torn by a crash has no end: without one here, this event
    // would be glued to it and skipped with it.
    let len = file.metadata()?.len();
    if len > 0 {
        let mut last = [0u8];
        file.seek(SeekFrom::Start(len - 1))?;
        file.read_exact(&mut last)?;
        if last[0] != b'\n' {
            line.insert(0, '\n');
        }
    }
    file.write_all(line.as_bytes())
}

/// Every event of the log at `path`, oldest first; none if there is no log.
pub fn read(path: &Path) -> Vec<TaskEvent> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    // Lossy: a torn last line can end inside a character.
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// The events worth keeping at `now_ms`: the last [`KEEP_DAYS`] of them,
/// and of those the newest [`KEEP_EVENTS`]. A card whose task was given
/// before that goes with it; what is left of its events touches nothing.
pub fn bound(events: Vec<TaskEvent>, now_ms: u64) -> Vec<TaskEvent> {
    let since = now_ms.saturating_sub(KEEP_DAYS * 24 * 3600 * 1000);
    let mut kept: Vec<TaskEvent> = events.into_iter().filter(|e| e.at_ms >= since).collect();
    let over = kept.len().saturating_sub(KEEP_EVENTS);
    kept.drain(..over);
    kept
}

/// Rewrites the log at `path` to what [`bound`] keeps, dropping any line
/// that does not parse. Meant for the app's start, before anything is
/// appended.
pub fn compact(path: &Path, now_ms: u64) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let mut text = String::new();
    for event in bound(read(path), now_ms) {
        text.push_str(&serde_json::to_string(&event)?);
        text.push('\n');
    }
    atomic_write(path, text.as_bytes())
}

/// The board the log adds up to: one card per task given, oldest first.
pub fn cards(events: &[TaskEvent]) -> Vec<TaskCard> {
    let mut cards: Vec<TaskCard> = Vec::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    for event in events {
        let at = event.at_ms;
        match &event.what {
            TaskEventKind::Given { card, source, from, to, pane, team, tab, text } => {
                if *source != TaskSource::Manual {
                    // The agent has moved on: whatever it was given before is over.
                    for older in cards.iter_mut().filter(|c| following(c) && c.pane == *pane) {
                        older.column = TaskColumn::Done;
                        older.updated_ms = at;
                        older.history.push(event.clone());
                    }
                }
                // A task sent for a card written by hand (`agent send
                // --card`) is that card's, which stays where it was put.
                if let Some(&i) = by_id.get(card) {
                    cards[i].updated_ms = at;
                    cards[i].history.push(event.clone());
                    continue;
                }
                let column = if *source == TaskSource::Manual { TaskColumn::Todo } else { TaskColumn::Doing };
                by_id.insert(card.clone(), cards.len());
                cards.push(TaskCard {
                    id: card.clone(),
                    title: text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string(),
                    source: *source,
                    from: from.clone(),
                    assignee: to.clone(),
                    pane: pane.clone(),
                    team: team.clone(),
                    tab: tab.clone(),
                    column,
                    created_ms: at,
                    updated_ms: at,
                    history: vec![event.clone()],
                });
            }
            TaskEventKind::Moved { card, column, .. } => {
                if let Some(c) = by_id.get(card).map(|&i| &mut cards[i]) {
                    c.column = *column;
                    c.updated_ms = at;
                    c.history.push(event.clone());
                }
            }
            TaskEventKind::TurnEnded { pane, end } => {
                let latest = cards
                    .iter_mut()
                    .rev()
                    .find(|c| following(c) && c.pane.as_deref() == Some(pane.as_str()));
                if let Some(c) = latest {
                    c.column = match end {
                        TurnEnd::NeedsYou => TaskColumn::NeedsYou,
                        TurnEnd::Done | TurnEnd::Exited => TaskColumn::Done,
                    };
                    c.updated_ms = at;
                    c.history.push(event.clone());
                }
            }
            TaskEventKind::Resumed { pane } => {
                let latest = cards
                    .iter_mut()
                    .rev()
                    .find(|c| following(c) && c.pane.as_deref() == Some(pane.as_str()));
                if let Some(c) = latest.filter(|c| c.column == TaskColumn::NeedsYou) {
                    c.column = TaskColumn::Doing;
                    c.updated_ms = at;
                    c.history.push(event.clone());
                }
            }
        }
    }
    cards
}

/// What an agent's status going from `before` to `after` adds to the log:
/// a turn ending as it stops or asks a person, a resume as it goes back to
/// work after asking. Starting work on a task is logged when the task is
/// given, not here.
pub fn status_event(pane: &str, before: Option<AgentStatus>, after: Option<AgentStatus>) -> Option<TaskEventKind> {
    use AgentStatus::*;
    let end = |end| Some(TaskEventKind::TurnEnded { pane: pane.to_string(), end });
    match (before?, after?) {
        (Working | NeedsYou, Idle) => end(TurnEnd::Done),
        (Working | Idle, NeedsYou) => end(TurnEnd::NeedsYou),
        (NeedsYou, Working) => Some(TaskEventKind::Resumed { pane: pane.to_string() }),
        _ => None,
    }
}

/// Which events a history is of.
#[derive(Debug, Clone, Copy)]
pub enum Scope<'a> {
    All,
    /// The tasks given in a tab, and what came of them.
    Tab(&'a str),
    /// The tasks given to the agent in `pane` or sent by it — by `name`,
    /// in `tab` — and its turns.
    Agent { pane: &'a str, name: Option<&'a str>, tab: Option<&'a str> },
}

/// The events of `scope`, oldest first, each turn end with how long the
/// turn took: since the task it worked on was given, or since it resumed.
pub fn history(events: &[TaskEvent], scope: Scope) -> Vec<TaskHistoryEntry> {
    let mut name_of: HashMap<&str, &str> = HashMap::new();
    let mut tab_of_pane: HashMap<&str, &str> = HashMap::new();
    let mut card_at: HashMap<&str, (Option<&str>, Option<&str>)> = HashMap::new();
    let mut started: HashMap<&str, u64> = HashMap::new();
    let mut out = Vec::new();
    for event in events {
        let (agent, turn_ms, keep) = match &event.what {
            TaskEventKind::Given { card, source, from, to, pane, tab, .. } => {
                let pane = pane.as_deref();
                card_at.insert(card, (pane, tab.as_deref()));
                if let Some(p) = pane {
                    // A card written for an agent is not typed into it: its
                    // turn has not started.
                    if *source != TaskSource::Manual {
                        started.insert(p, event.at_ms);
                    }
                    if let Some(to) = to {
                        name_of.insert(p, to);
                    }
                    if let Some(t) = tab {
                        tab_of_pane.insert(p, t);
                    }
                }
                let keep = match scope {
                    Scope::All => true,
                    Scope::Tab(t) => tab.as_deref() == Some(t),
                    Scope::Agent { pane: p, name, tab: t } => {
                        pane == Some(p) || (name == Some(from.as_str()) && tab.as_deref() == t)
                    }
                };
                (None, None, keep)
            }
            TaskEventKind::Moved { card, .. } => {
                let (pane, tab) = card_at.get(card.as_str()).copied().unwrap_or_default();
                let keep = match scope {
                    Scope::All => true,
                    Scope::Tab(t) => tab == Some(t),
                    Scope::Agent { pane: p, .. } => pane == Some(p),
                };
                (None, None, keep)
            }
            TaskEventKind::TurnEnded { pane, .. } | TaskEventKind::Resumed { pane } => {
                let turn_ms = if matches!(event.what, TaskEventKind::Resumed { .. }) {
                    started.insert(pane, event.at_ms);
                    None
                } else {
                    started.remove(pane.as_str()).map(|s| event.at_ms.saturating_sub(s))
                };
                let keep = match scope {
                    Scope::All => true,
                    Scope::Tab(t) => tab_of_pane.get(pane.as_str()) == Some(&t),
                    Scope::Agent { pane: p, .. } => pane == p,
                };
                (name_of.get(pane.as_str()).map(|n| n.to_string()), turn_ms, keep)
            }
        };
        if keep {
            let summary = summary(&event.what, agent.as_deref(), turn_ms);
            out.push(TaskHistoryEntry { event: event.clone(), agent, turn_ms, summary });
        }
    }
    out
}

/// The card `id` names — the whole id, or a prefix of one card's only —
/// if it was written by hand, as only those are moved by hand or have
/// tasks sent for them. A card Mirador made follows its agent: a move
/// would only be undone by its next turn.
pub fn written<'a>(cards: &'a [TaskCard], id: &str) -> Result<&'a TaskCard, String> {
    let card = find(cards, id)?;
    if card.source != TaskSource::Manual {
        return Err(format!(
            "card {} follows {}: only a card written with `mira task add` is moved or sent for by hand",
            card.id,
            card.assignee.as_deref().unwrap_or("its agent")
        ));
    }
    Ok(card)
}

/// The card `id` names: the whole id, or a prefix of one card's only.
pub fn find<'a>(cards: &'a [TaskCard], id: &str) -> Result<&'a TaskCard, String> {
    let id = id.trim();
    let hits: Vec<&TaskCard> = match cards.iter().find(|c| c.id == id) {
        Some(exact) => vec![exact],
        None if id.is_empty() => Vec::new(),
        None => cards.iter().filter(|c| c.id.starts_with(id)).collect(),
    };
    let card = match hits.as_slice() {
        [] => return Err(format!("no card `{id}` (`mira task list` shows each card's id)")),
        [one] => *one,
        many => {
            let ids: Vec<&str> = many.iter().map(|c| c.id.as_str()).collect();
            return Err(format!("`{id}` is more than one card: {}", ids.join(", ")));
        }
    };
    Ok(card)
}

/// The history of the card `id` names: the lines of [`history`] for the
/// events that touched it, oldest first.
pub fn card_history(events: &[TaskEvent], id: &str) -> Result<Vec<TaskHistoryEntry>, String> {
    let cards = cards(events);
    let card = find(&cards, id)?;
    Ok(history(events, Scope::All).into_iter().filter(|e| card.history.contains(&e.event)).collect())
}

/// One event in a line, after its age: who to whom and what, or how a
/// turn ended and how long it took.
fn summary(what: &TaskEventKind, agent: Option<&str>, turn_ms: Option<u64>) -> String {
    let agent = agent.unwrap_or("-");
    match what {
        TaskEventKind::Given { source, from, to, text, .. } => {
            let source = match source {
                TaskSource::TeamStart => "team-start",
                TaskSource::AgentNew => "agent-new",
                TaskSource::AgentSend => "agent-send",
                TaskSource::Manual => "manual",
            };
            let title = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
            format!("{from} → {}  {source}  {title}", to.as_deref().unwrap_or("-"))
        }
        TaskEventKind::Moved { card, column, by } => format!("{by} moved {card} to {}", column_name(*column)),
        TaskEventKind::TurnEnded { end, .. } => {
            let end = match end {
                TurnEnd::Done => "done",
                TurnEnd::NeedsYou => "needs-you",
                TurnEnd::Exited => "exited",
            };
            match turn_ms {
                Some(ms) => format!("{agent} {end} after {}", span(ms)),
                None => format!("{agent} {end}"),
            }
        }
        TaskEventKind::Resumed { .. } => format!("{agent} resumed"),
    }
}

/// What a card calls the agent in a pane, as `agent list` does: its name
/// in its team, its session's, its role's — or, for a plain agent with
/// none of those, its harness's (`claude`, `codex`).
pub fn agent_name(member: Option<String>, meta: Option<&crate::state::PaneMeta>) -> Option<String> {
    member.or_else(|| {
        let m = meta?;
        m.agent_name.clone().or_else(|| m.agent_role.clone()).or_else(|| {
            serde_json::to_value(m.harness()?).ok()?.as_str().map(String::from)
        })
    })
}

/// A column by the name the CLI takes and shows: `todo`, `doing`,
/// `needs-you`, `done`.
pub fn column_name(column: TaskColumn) -> &'static str {
    match column {
        TaskColumn::Todo => "todo",
        TaskColumn::Doing => "doing",
        TaskColumn::NeedsYou => "needs-you",
        TaskColumn::Done => "done",
    }
}

pub fn parse_column(name: &str) -> Result<TaskColumn, String> {
    [TaskColumn::Todo, TaskColumn::Doing, TaskColumn::NeedsYou, TaskColumn::Done]
        .into_iter()
        .find(|c| column_name(*c) == name)
        .ok_or_else(|| format!("no column `{name}` (todo, doing, needs-you, done)"))
}

/// `cards` as a board shows them: every card not done, and of those done
/// the `done` most recently moved, so the column does not grow forever.
pub fn recent(cards: Vec<TaskCard>, done: usize) -> Vec<TaskCard> {
    let mut finished: Vec<(u64, String)> =
        cards.iter().filter(|c| c.column == TaskColumn::Done).map(|c| (c.updated_ms, c.id.clone())).collect();
    finished.sort_unstable_by(|a, b| b.cmp(a));
    finished.truncate(done);
    cards
        .into_iter()
        .filter(|c| c.column != TaskColumn::Done || finished.iter().any(|(_, id)| *id == c.id))
        .collect()
}

/// A length of time as the CLI shows it: `45s`, `12m`, `3h`, `2d`.
pub fn span(ms: u64) -> String {
    let s = ms / 1000;
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

/// Whether `card` still follows its agent: given by Mirador, and not over.
fn following(card: &TaskCard) -> bool {
    card.source != TaskSource::Manual && matches!(card.column, TaskColumn::Doing | TaskColumn::NeedsYou)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn given(at_ms: u64, card: &str, source: TaskSource, to: &str, text: &str) -> TaskEvent {
        TaskEvent {
            at_ms,
            what: TaskEventKind::Given {
                card: card.into(),
                source,
                from: "you".into(),
                to: Some(to.into()),
                pane: Some(format!("pane-{to}")),
                team: Some("omakase".into()),
                tab: Some("tab-1".into()),
                text: text.into(),
            },
        }
    }

    fn moved(at_ms: u64, card: &str, column: TaskColumn) -> TaskEvent {
        TaskEvent { at_ms, what: TaskEventKind::Moved { card: card.into(), column, by: "planner".into() } }
    }

    fn ended(at_ms: u64, to: &str, end: TurnEnd) -> TaskEvent {
        TaskEvent { at_ms, what: TaskEventKind::TurnEnded { pane: format!("pane-{to}"), end } }
    }

    fn columns(cards: &[TaskCard]) -> Vec<(&str, TaskColumn)> {
        cards.iter().map(|c| (c.id.as_str(), c.column)).collect()
    }

    fn temp_log(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mira-tasks-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("tasks.jsonl")
    }

    #[test]
    fn a_task_mirador_gives_is_a_card_in_doing() {
        for source in [TaskSource::TeamStart, TaskSource::AgentNew, TaskSource::AgentSend] {
            let cards = cards(&[given(100, "a", source, "worker", "\n  Step 1: the model  \nthen more")]);
            let [card] = cards.as_slice() else { panic!("{cards:?}") };
            assert_eq!(card.column, TaskColumn::Doing, "{source:?}");
            assert_eq!(card.title, "Step 1: the model");
            assert_eq!(card.source, source);
            assert_eq!(card.from, "you");
            assert_eq!(card.assignee.as_deref(), Some("worker"));
            assert_eq!(card.pane.as_deref(), Some("pane-worker"));
            assert_eq!((card.team.as_deref(), card.tab.as_deref()), (Some("omakase"), Some("tab-1")));
            assert_eq!((card.created_ms, card.updated_ms), (100, 100));
            assert_eq!(card.history.len(), 1);
        }
    }

    #[test]
    fn a_newer_task_to_the_same_agent_closes_the_older_card() {
        let cards = cards(&[
            given(100, "a", TaskSource::TeamStart, "worker", "first"),
            given(150, "other", TaskSource::AgentSend, "reviewer", "look"),
            given(200, "b", TaskSource::AgentSend, "worker", "second"),
        ]);
        assert_eq!(
            columns(&cards),
            [("a", TaskColumn::Done), ("other", TaskColumn::Doing), ("b", TaskColumn::Doing)]
        );
        assert_eq!(cards[0].updated_ms, 200);
        // Its history says what closed it.
        assert_eq!(cards[0].history.last(), cards[2].history.first());
    }

    #[test]
    fn a_card_follows_its_agents_turns() {
        let log = [
            given(100, "a", TaskSource::AgentSend, "worker", "do it"),
            ended(200, "worker", TurnEnd::NeedsYou),
        ];
        assert_eq!(columns(&cards(&log)), [("a", TaskColumn::NeedsYou)]);
        let log = [log.to_vec(), vec![ended(300, "worker", TurnEnd::Done)]].concat();
        let cards = cards(&log);
        assert_eq!(columns(&cards), [("a", TaskColumn::Done)]);
        assert_eq!(cards[0].updated_ms, 300);
        assert_eq!(cards[0].history, log);
        // A turn ending later touches no card already done.
        let later = cards_after(&log, ended(400, "worker", TurnEnd::NeedsYou));
        assert_eq!(columns(&later), [("a", TaskColumn::Done)]);
        assert_eq!(later[0].history.len(), 3);
        // An agent that exits ends its card too.
        let exited = cards_after(&log[..1], ended(250, "worker", TurnEnd::Exited));
        assert_eq!(columns(&exited), [("a", TaskColumn::Done)]);
    }

    #[test]
    fn a_card_goes_back_to_doing_when_its_agent_resumes() {
        let log = [
            given(100, "a", TaskSource::AgentSend, "worker", "do it"),
            given(110, "m", TaskSource::Manual, "worker", "written"),
            ended(200, "worker", TurnEnd::NeedsYou),
            resumed(250, "worker"),
        ];
        let cards = cards(&log);
        assert_eq!(columns(&cards), [("a", TaskColumn::Doing), ("m", TaskColumn::Todo)]);
        assert_eq!(cards[0].updated_ms, 250);
        assert_eq!(cards[0].history, [log[0].clone(), log[2].clone(), log[3].clone()]);
        let done = cards_after(&log, ended(300, "worker", TurnEnd::Done));
        assert_eq!(columns(&done), [("a", TaskColumn::Done), ("m", TaskColumn::Todo)]);
        assert_eq!(done[0].history.len(), 4);
        // Resuming touches no card that was not waiting on a person.
        let busy = super::cards(&[log[0].clone(), resumed(150, "worker")]);
        assert_eq!((busy[0].column, busy[0].history.len()), (TaskColumn::Doing, 1));
    }

    fn resumed(at_ms: u64, to: &str) -> TaskEvent {
        TaskEvent { at_ms, what: TaskEventKind::Resumed { pane: format!("pane-{to}") } }
    }

    fn cards_after(log: &[TaskEvent], event: TaskEvent) -> Vec<TaskCard> {
        cards(&[log.to_vec(), vec![event]].concat())
    }

    #[test]
    fn a_written_card_moves_only_when_moved() {
        let log = [
            given(100, "m", TaskSource::Manual, "worker", "Step 2: record events"),
            ended(150, "worker", TurnEnd::Done),
            given(200, "a", TaskSource::AgentSend, "worker", "go"),
        ];
        assert_eq!(columns(&cards(&log)), [("m", TaskColumn::Todo), ("a", TaskColumn::Doing)]);
        let log = [log.to_vec(), vec![moved(300, "m", TaskColumn::Doing), moved(400, "m", TaskColumn::Done)]].concat();
        let cards = cards(&log);
        assert_eq!(columns(&cards), [("m", TaskColumn::Done), ("a", TaskColumn::Doing)]);
        assert_eq!((cards[0].updated_ms, cards[0].history.len()), (400, 3));
        // A move of a card the log no longer has is nothing.
        assert!(super::cards(&[moved(1, "gone", TaskColumn::Done)]).is_empty());
    }

    #[test]
    fn the_log_survives_a_line_torn_by_a_crash() {
        let path = temp_log("torn");
        append(&path, &given(100, "a", TaskSource::AgentSend, "worker", "first")).unwrap();
        // A crash mid-write: half a line, ending inside a character.
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(&"{\"atMs\":150,\"kind\":\"given\",\"text\":\"é".as_bytes()[..36]).unwrap();
        assert_eq!(read(&path).len(), 1);
        // The next event starts a line of its own rather than joining it.
        append(&path, &ended(200, "worker", TurnEnd::Done)).unwrap();
        let events = read(&path);
        assert_eq!(events, [given(100, "a", TaskSource::AgentSend, "worker", "first"), ended(200, "worker", TurnEnd::Done)]);
        assert!(read(&temp_log("missing")).is_empty());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_log_keeps_thirty_days_and_ten_thousand_events() {
        let day = 24 * 3600 * 1000;
        let now = 100 * day;
        let old = given(now - 31 * day, "old", TaskSource::AgentSend, "worker", "old");
        let recent = given(now - 29 * day, "recent", TaskSource::AgentSend, "worker", "recent");
        assert_eq!(bound(vec![old.clone(), recent.clone()], now), std::slice::from_ref(&recent));
        let many: Vec<TaskEvent> = (0..KEEP_EVENTS as u64 + 5).map(|i| ended(now - day + i, "worker", TurnEnd::Done)).collect();
        let kept = bound(many.clone(), now);
        assert_eq!(kept.len(), KEEP_EVENTS);
        assert_eq!(kept[0], many[5]);

        let path = temp_log("compact");
        for event in [&old, &recent] {
            append(&path, event).unwrap();
        }
        std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(b"{torn").unwrap();
        compact(&path, now).unwrap();
        assert_eq!(read(&path), [recent]);
        assert!(std::fs::read_to_string(&path).unwrap().ends_with("}\n"));
        compact(&temp_log("absent"), now).unwrap();
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn events_are_camel_case_lines() {
        let line = serde_json::to_string(&ended(5, "worker", TurnEnd::NeedsYou)).unwrap();
        assert_eq!(line, r#"{"atMs":5,"kind":"turnEnded","pane":"pane-worker","end":"needsYou"}"#);
        let line = serde_json::to_string(&moved(6, "m", TaskColumn::NeedsYou)).unwrap();
        assert_eq!(line, r#"{"atMs":6,"kind":"moved","card":"m","column":"needsYou","by":"planner"}"#);
        assert_eq!(serde_json::from_str::<TaskEvent>(&line).unwrap(), moved(6, "m", TaskColumn::NeedsYou));
    }

    #[test]
    fn a_status_change_ends_or_resumes_a_turn() {
        use AgentStatus::*;
        let at = |before, after| status_event("p", before, after);
        let ended = |end| Some(TaskEventKind::TurnEnded { pane: "p".into(), end });
        assert_eq!(at(Some(Working), Some(Idle)), ended(TurnEnd::Done));
        assert_eq!(at(Some(NeedsYou), Some(Idle)), ended(TurnEnd::Done));
        assert_eq!(at(Some(Working), Some(NeedsYou)), ended(TurnEnd::NeedsYou));
        assert_eq!(at(Some(NeedsYou), Some(Working)), Some(TaskEventKind::Resumed { pane: "p".into() }));
        // Starting a turn is the task's; an unknown status says nothing.
        assert_eq!(at(Some(Idle), Some(Working)), None);
        assert_eq!(at(None, Some(Idle)), None);
        assert_eq!(at(Some(Working), None), None);
    }

    #[test]
    fn history_says_how_long_each_turn_took() {
        let mut sent = given(5_000, "b", TaskSource::AgentSend, "reviewer", "Review 3505520\nplease");
        if let TaskEventKind::Given { from, .. } = &mut sent.what {
            *from = "worker".into();
        }
        let log = [
            given(1_000, "a", TaskSource::TeamStart, "worker", "Step 1"),
            ended(3_000, "worker", TurnEnd::NeedsYou),
            resumed(4_000, "worker"),
            sent,
            ended(64_000, "worker", TurnEnd::Done),
            ended(70_000, "reviewer", TurnEnd::Done),
            given(80_000, "x", TaskSource::AgentSend, "elsewhere", "other tab"),
        ];
        let mut other = log.to_vec();
        if let TaskEventKind::Given { tab, .. } = &mut other[6].what {
            *tab = Some("tab-2".into());
        }
        let all = history(&other, Scope::All);
        assert_eq!(all.len(), 7);
        let turns: Vec<(Option<&str>, Option<u64>)> = all.iter().map(|e| (e.agent.as_deref(), e.turn_ms)).collect();
        assert_eq!(
            turns,
            [
                (None, None),
                (Some("worker"), Some(2_000)),
                (Some("worker"), None),
                (None, None),
                (Some("worker"), Some(60_000)),
                (Some("reviewer"), Some(65_000)),
                (None, None),
            ]
        );
        assert_eq!(history(&other, Scope::Tab("tab-1")).len(), 6);
        // An agent's: what it was given, what it sent, its own turns.
        let worker = history(&other, Scope::Agent { pane: "pane-worker", name: Some("worker"), tab: Some("tab-1") });
        let at: Vec<u64> = worker.iter().map(|e| e.event.at_ms).collect();
        assert_eq!(at, [1_000, 3_000, 4_000, 5_000, 64_000]);
    }

    #[test]
    fn only_a_written_card_is_moved_or_sent_for() {
        let cards = cards(&[
            given(1, "ab12cd34", TaskSource::Manual, "worker", "Step 1"),
            given(2, "ab99ffff", TaskSource::Manual, "worker", "Step 2"),
            given(3, "77777777", TaskSource::AgentSend, "worker", "go"),
        ]);
        assert_eq!(written(&cards, "ab12cd34").unwrap().title, "Step 1");
        assert_eq!(written(&cards, " ab9 ").unwrap().title, "Step 2");
        let two = written(&cards, "ab").unwrap_err();
        assert!(two.contains("more than one") && two.contains("ab12cd34, ab99ffff"), "{two}");
        assert!(written(&cards, "zz").unwrap_err().contains("no card `zz`"));
        assert!(written(&cards, "").unwrap_err().contains("no card"));
        let auto = written(&cards, "777").unwrap_err();
        assert!(auto.contains("follows worker"), "{auto}");
    }

    #[test]
    fn a_task_sent_for_a_written_card_is_that_cards() {
        let log = [
            given(100, "auto", TaskSource::AgentSend, "worker", "earlier"),
            given(200, "m", TaskSource::Manual, "worker", "Step 2: the board"),
            moved(300, "m", TaskColumn::Doing),
            given(400, "m", TaskSource::AgentSend, "worker", "Step 2 (card m): build the board"),
            ended(500, "worker", TurnEnd::Done),
        ];
        let cards = cards(&log);
        assert_eq!(columns(&cards), [("auto", TaskColumn::Done), ("m", TaskColumn::Doing)]);
        let m = &cards[1];
        assert_eq!((m.title.as_str(), m.source, m.updated_ms), ("Step 2: the board", TaskSource::Manual, 400));
        assert_eq!(m.history, log[1..4]);
    }

    #[test]
    fn a_board_shows_only_the_latest_done() {
        let log: Vec<TaskEvent> = (0..5)
            .map(|i| given(i * 10, &format!("m{i}"), TaskSource::Manual, "worker", "step"))
            .chain([moved(100, "m3", TaskColumn::Done), moved(200, "m0", TaskColumn::Done), moved(300, "m1", TaskColumn::Done)])
            .collect();
        let shown: Vec<String> = recent(cards(&log), 2).into_iter().map(|c| c.id).collect();
        assert_eq!(shown, ["m0", "m1", "m2", "m4"]);
        assert_eq!(recent(cards(&log), 20).len(), 5);
    }

    #[test]
    fn columns_go_by_their_cli_names() {
        for column in [TaskColumn::Todo, TaskColumn::Doing, TaskColumn::NeedsYou, TaskColumn::Done] {
            assert_eq!(parse_column(column_name(column)), Ok(column));
        }
        assert!(parse_column("Doing").unwrap_err().contains("todo, doing, needs-you, done"));
    }

    #[test]
    fn a_card_written_for_an_agent_starts_no_turn() {
        let log = [
            given(1_000, "a", TaskSource::AgentSend, "worker", "go"),
            given(5_000, "m", TaskSource::Manual, "worker", "Step 2"),
            ended(9_000, "worker", TurnEnd::Done),
        ];
        assert_eq!(history(&log, Scope::All)[2].turn_ms, Some(8_000));
    }

    #[test]
    fn history_lines_read_as_the_cli_prints_them() {
        let log = [
            given(1_000, "a", TaskSource::AgentSend, "worker", "\nStep 2: the board\nwith tests"),
            moved(2_000, "a", TaskColumn::Done),
            ended(64_000, "worker", TurnEnd::Done),
            resumed(65_000, "worker"),
            ended(66_000, "nobody", TurnEnd::Exited),
        ];
        let lines: Vec<String> = history(&log, Scope::All).into_iter().map(|e| e.summary).collect();
        assert_eq!(
            lines,
            [
                "you → worker  agent-send  Step 2: the board",
                "planner moved a to done",
                "worker done after 1m",
                "worker resumed",
                "- exited",
            ]
        );
    }

    #[test]
    fn a_cards_history_is_the_events_that_touched_it() {
        let log = [
            given(1_000, "m", TaskSource::Manual, "worker", "Step 2"),
            given(2_000, "other", TaskSource::AgentSend, "reviewer", "look"),
            given(3_000, "m", TaskSource::AgentSend, "worker", "Step 2 (card m): go"),
            ended(9_000, "worker", TurnEnd::Done),
            moved(10_000, "m", TaskColumn::Done),
        ];
        let lines: Vec<String> = card_history(&log, "m").unwrap().into_iter().map(|e| e.summary).collect();
        assert_eq!(
            lines,
            ["you → worker  manual  Step 2", "you → worker  agent-send  Step 2 (card m): go", "planner moved m to done"]
        );
        // The automatic card it was sent on would have had the turn end;
        // the written card does not follow turns.
        let auto = card_history(&log, "oth").unwrap();
        assert_eq!(auto.len(), 1);
        assert!(card_history(&log, "zz").unwrap_err().contains("no card `zz`"));
    }

    #[test]
    fn a_plain_agent_is_called_by_its_harness() {
        use crate::state::PaneMeta;
        let plain = PaneMeta { agent_launched: true, ..Default::default() };
        assert_eq!(agent_name(None, Some(&plain)).as_deref(), Some("claude"));
        let codex = PaneMeta { agent_harness: Some(cmux_protocol::Harness::Codex), ..Default::default() };
        assert_eq!(agent_name(None, Some(&codex)).as_deref(), Some("codex"));
        let named = PaneMeta { agent_name: Some("fix-login".into()), agent_role: Some("dev".into()), ..plain.clone() };
        assert_eq!(agent_name(None, Some(&named)).as_deref(), Some("fix-login"));
        assert_eq!(agent_name(Some("worker".into()), Some(&named)).as_deref(), Some("worker"));
        assert_eq!(agent_name(None, Some(&PaneMeta::default())), None);
        assert_eq!(agent_name(None, None), None);
    }

    #[test]
    fn spans_are_short() {
        assert_eq!(span(999), "0s");
        assert_eq!(span(59_000), "59s");
        assert_eq!(span(60_000), "1m");
        assert_eq!(span(3 * 3_600_000 + 1), "3h");
        assert_eq!(span(50 * 3_600_000), "2d");
    }
}
