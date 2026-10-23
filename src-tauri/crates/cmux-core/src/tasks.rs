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

use cmux_protocol::{TaskCard, TaskColumn, TaskEvent, TaskEventKind, TaskSource, TurnEnd};

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
                if by_id.contains_key(card) {
                    continue;
                }
                let column = if *source == TaskSource::Manual {
                    TaskColumn::Todo
                } else {
                    // The agent has moved on: whatever it was given before is over.
                    for older in cards.iter_mut().filter(|c| following(c) && c.pane == *pane) {
                        older.column = TaskColumn::Done;
                        older.updated_ms = at;
                        older.history.push(event.clone());
                    }
                    TaskColumn::Doing
                };
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
}
