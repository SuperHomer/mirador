//! The task log in the app: what it records, and the board and history it
//! answers with. The log's format and the fold into cards are
//! `cmux_core::tasks`'s; this is where they meet panes, tabs and teams.

use cmux_protocol::{TaskCard, TaskColumn, TaskEvent, TaskEventKind, TaskHistoryEntry, TaskSource};

use crate::AppState;

/// Adds `what` to the log, now. The log's lock is held for the append
/// alone, so events go in one at a time and in order.
pub fn record(state: &AppState, what: TaskEventKind) {
    let event = TaskEvent { at_ms: crate::runs::now_ms(), what };
    let path = state.task_log.lock().unwrap();
    if let Err(e) = cmux_core::tasks::append(&path, &event) {
        eprintln!("mirador: could not log a task: {e}");
    }
}

/// Logs `text` as given to the agent in `pane` by `from`, with its name as
/// `agent list` shows it, and its tab and team. Nothing for an empty text.
pub fn given(state: &AppState, source: TaskSource, from: String, pane: &str, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    let (tab, team, to) = whereabouts(state, pane);
    record(
        state,
        TaskEventKind::Given {
            card: cmux_core::tasks::new_id(),
            source,
            from,
            to,
            pane: Some(pane.to_string()),
            team,
            tab,
            text: text.to_string(),
        },
    );
}

/// Writes a card titled `title` in `caller`'s tab, for the agent `to`
/// names there — which must be one — or for nobody yet. Its id.
pub fn add(
    state: &AppState,
    title: &str,
    to: Option<String>,
    caller: Option<String>,
    from: String,
) -> Result<String, String> {
    if title.trim().is_empty() {
        return Err("the card has no title".into());
    }
    let caller = caller.unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    let pane = match to {
        Some(to) => Some(crate::handoff::find(state, &to, Some(caller.clone()))?),
        None => None,
    };
    let (tab, team, _) = whereabouts(state, &caller);
    let to = pane.as_deref().and_then(|p| whereabouts(state, p).2);
    let card = cmux_core::tasks::new_id();
    record(
        state,
        TaskEventKind::Given {
            card: card.clone(),
            source: TaskSource::Manual,
            from,
            to,
            pane,
            team,
            tab,
            text: title.trim().to_string(),
        },
    );
    Ok(card)
}

/// Puts the card written by hand that `id` names in `column`.
pub fn move_card(state: &AppState, id: &str, column: TaskColumn, by: String) -> Result<(), String> {
    let card = cmux_core::tasks::movable(&cmux_core::tasks::cards(&events(state)), id)?.id.clone();
    record(state, TaskEventKind::Moved { card, column, by });
    Ok(())
}

/// The tab and team of `pane`, and its name as `agent list` shows it.
fn whereabouts(state: &AppState, pane: &str) -> (Option<String>, Option<String>, Option<String>) {
    let (tab, team, member) = {
        let ws = state.workspace.lock().unwrap();
        let tab = ws.tab_of_pane(pane);
        let run = tab.and_then(|t| t.team.as_ref());
        (
            tab.map(|t| t.id.clone()),
            run.map(|r| r.name.clone()),
            run.and_then(|r| r.members.iter().find(|s| s.pane == pane)).map(|s| s.name.clone()),
        )
    };
    let to = member.or_else(|| {
        let meta = state.meta.lock().unwrap();
        let m = meta.get(pane)?;
        m.agent_name.clone().or_else(|| m.agent_role.clone())
    });
    (tab, team, to)
}

/// The tab `pane_id` (the focused pane when absent) is in.
fn tab_of(state: &AppState, pane_id: Option<String>) -> Result<String, String> {
    let ws = state.workspace.lock().unwrap();
    let pane = pane_id.unwrap_or_else(|| ws.focused_pane());
    ws.tab_of_pane(&pane).map(|t| t.id.clone()).ok_or_else(|| format!("no pane {pane}"))
}

fn events(state: &AppState) -> Vec<TaskEvent> {
    let path = state.task_log.lock().unwrap();
    cmux_core::tasks::read(&path)
}

/// The cards of `pane_id`'s tab — its team's, if it is a team's — or
/// every card with `all`.
pub fn list(state: &AppState, all: bool, pane_id: Option<String>) -> Result<Vec<TaskCard>, String> {
    let tab = if all { None } else { Some(tab_of(state, pane_id)?) };
    Ok(cmux_core::tasks::cards(&events(state))
        .into_iter()
        .filter(|c| tab.is_none() || c.tab == tab)
        .collect())
}

/// The log of the agent `name` from `pane_id`'s tab, else of that tab,
/// or all of it with `all`.
pub fn history(
    state: &AppState,
    name: Option<String>,
    all: bool,
    pane_id: Option<String>,
) -> Result<Vec<TaskHistoryEntry>, String> {
    let events = events(state);
    if all {
        return Ok(cmux_core::tasks::history(&events, cmux_core::tasks::Scope::All));
    }
    let Some(name) = name else {
        let tab = tab_of(state, pane_id)?;
        return Ok(cmux_core::tasks::history(&events, cmux_core::tasks::Scope::Tab(&tab)));
    };
    let pane = crate::handoff::find(state, &name, pane_id)?;
    let tab = tab_of(state, Some(pane.clone())).ok();
    let sender = crate::handoff::sender(state, &pane);
    let scope = cmux_core::tasks::Scope::Agent { pane: &pane, name: sender.as_deref(), tab: tab.as_deref() };
    Ok(cmux_core::tasks::history(&events, scope))
}
