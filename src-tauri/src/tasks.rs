//! The task log in the app: what it records, and the board and history it
//! answers with. The log's format and the fold into cards are
//! `cmux_core::tasks`'s; this is where they meet panes, tabs and teams.

use cmux_protocol::{TaskCard, TaskColumn, TaskEvent, TaskEventKind, TaskHistoryEntry, TaskSource};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::AppState;

/// How many done cards a board and `task list` show: the most recently
/// moved.
const BOARD_DONE: usize = 20;

/// Adds `what` to the log, now, and tells the boards. The log's lock is
/// held for the append alone, so events go in one at a time and in order.
pub fn record(app: &AppHandle, what: TaskEventKind) {
    let event = TaskEvent { at_ms: crate::runs::now_ms(), what };
    {
        let state = app.state::<AppState>();
        let path = state.task_log.lock().unwrap();
        if let Err(e) = cmux_core::tasks::append(&path, &event) {
            eprintln!("mirador: could not log a task: {e}");
            return;
        }
    }
    let _ = app.emit("tasks-changed", ());
}

/// Logs `text` as given to the agent in `pane` by `from`, with its name as
/// `agent list` shows it, and its tab and team: on `card`, a card written
/// by hand, or else a card of its own, titled `title` when given (else
/// from the text). Nothing for an empty text.
pub fn given(
    app: &AppHandle,
    source: TaskSource,
    from: String,
    pane: &str,
    text: &str,
    card: Option<String>,
    title: Option<String>,
) {
    if text.trim().is_empty() {
        return;
    }
    let (tab, team, to) = whereabouts(&app.state::<AppState>(), pane);
    record(
        app,
        TaskEventKind::Given {
            card: card.unwrap_or_else(cmux_core::tasks::new_id),
            source,
            from,
            to,
            pane: Some(pane.to_string()),
            team,
            tab,
            text: text.to_string(),
            title: title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()),
            description: None,
        },
    );
}

/// Writes a card titled `title`, with `description` when given, in
/// `caller`'s tab, for the agent `to` names there — which must be one — or
/// for nobody yet. Its id.
pub fn add(
    app: &AppHandle,
    title: &str,
    description: Option<String>,
    to: Option<String>,
    caller: Option<String>,
    from: String,
) -> Result<String, String> {
    if title.trim().is_empty() {
        return Err("the card has no title".into());
    }
    let state = app.state::<AppState>();
    let state = &*state;
    let caller = caller.unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    let pane = match to {
        Some(to) => Some(crate::handoff::find(state, &to, Some(caller.clone()))?),
        None => None,
    };
    let (tab, team, _) = whereabouts(state, &caller);
    let to = pane.as_deref().and_then(|p| whereabouts(state, p).2);
    let card = cmux_core::tasks::new_id();
    record(
        app,
        TaskEventKind::Given {
            card: card.clone(),
            source: TaskSource::Manual,
            from,
            to,
            pane,
            team,
            tab,
            text: title.trim().to_string(),
            title: None,
            description: description.map(|d| d.trim().to_string()).filter(|d| !d.is_empty()),
        },
    );
    Ok(card)
}

/// Puts the card written by hand that `id` names in `column`.
pub fn move_card(app: &AppHandle, id: &str, column: TaskColumn, by: String) -> Result<(), String> {
    let card = written(&app.state::<AppState>(), id)?;
    record(app, TaskEventKind::Moved { card, column, by });
    Ok(())
}

/// The whole id of the card written by hand that `id` names.
pub fn written(state: &AppState, id: &str) -> Result<String, String> {
    Ok(cmux_core::tasks::written(&cmux_core::tasks::cards(&events(state)), id)?.id.clone())
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
    let to = cmux_core::tasks::agent_name(member, state.meta.lock().unwrap().get(pane));
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
/// every card with `all`, as a board shows them.
pub fn list(state: &AppState, all: bool, pane_id: Option<String>) -> Result<Vec<TaskCard>, String> {
    let tab = if all { None } else { Some(tab_of(state, pane_id)?) };
    Ok(board(state, tab))
}

/// The cards of the tab `tab`, or of every tab: every one not done, and
/// the latest done. A card in Doing says how many actions failed in its
/// agent's turn so far.
fn board(state: &AppState, tab: Option<String>) -> Vec<TaskCard> {
    let cards = cmux_core::tasks::cards(&events(state))
        .into_iter()
        .filter(|c| tab.is_none() || c.tab == tab)
        .collect();
    let mut cards = cmux_core::tasks::recent(cards, BOARD_DONE);
    for card in cards.iter_mut().filter(|c| c.column == TaskColumn::Doing) {
        if let Some(pane) = card.pane.clone() {
            card.failing = failing(state, &pane);
        }
    }
    cards
}

/// The failed actions in the latest turn of the agent in `pane`, from its
/// trail — read through the trail's caches, which read only what the file
/// gained — or 0 when it has none to read.
fn failing(state: &AppState, pane: &str) -> u32 {
    use crate::commands::TrailFile;
    match crate::commands::trail_file(state, pane) {
        Ok(TrailFile::Claude(path)) => {
            state.trails.lock().unwrap().read(&path).map_or(0, |t| cmux_core::trail::failing(t.turns()))
        }
        Ok(TrailFile::Codex(known)) => {
            state.codex_trails.lock().unwrap().read(&known.path).map_or(0, |t| cmux_core::trail::failing(t.turns()))
        }
        Err(_) => 0,
    }
}

/// Writes a card in the tab `tab_id`, from the board: as `task add` does,
/// for the agent `assignee` names there or nobody. Its id.
#[tauri::command]
pub fn task_add(
    app: AppHandle,
    state: State<'_, AppState>,
    tab_id: String,
    title: String,
    description: Option<String>,
    assignee: Option<String>,
) -> Result<String, String> {
    let caller = {
        let ws = state.workspace.lock().unwrap();
        ws.tabs.iter().find(|t| t.id == tab_id).map(|t| t.focused.clone())
    }
    .ok_or_else(|| format!("no tab {tab_id}"))?;
    add(&app, &title, description, assignee, Some(caller), "you".into())
}

/// Sends the card written by hand `card` to the agent in `pane_id` as its
/// next prompt, as `agent send --card` does — refused for an agent waiting
/// on a person — and puts the card in Doing.
#[tauri::command]
pub fn task_send(app: AppHandle, state: State<'_, AppState>, card: String, pane_id: String) -> Result<String, String> {
    let cards = cmux_core::tasks::cards(&events(&state));
    let card = cmux_core::tasks::written(&cards, &card)?.clone();
    send_card(&app, &card, &pane_id)
}

/// Sends the agent in `pane_id` its next task: its own first To do card,
/// else the first in its tab that is no one's (`tasks::next_task`).
#[tauri::command]
pub fn send_next_task(app: AppHandle, state: State<'_, AppState>, pane_id: String) -> Result<String, String> {
    let tab = tab_of(&state, Some(pane_id.clone()))?;
    let cards = cmux_core::tasks::cards(&events(&state));
    let card = cmux_core::tasks::next_task(&cards, &pane_id, Some(&tab))
        .cloned()
        .ok_or("no card in To do for this agent, or for no one in its tab")?;
    send_card(&app, &card, &pane_id)
}

/// Types `card`'s title, and its description when it has one, into the
/// agent in `pane` (logged on the card), then puts the card in Doing.
fn send_card(app: &AppHandle, card: &TaskCard, pane: &str) -> Result<String, String> {
    let text = match &card.description {
        Some(more) => format!("{}\n\n{more}", card.title),
        None => card.title.clone(),
    };
    let pane = crate::handoff::send(app, pane, &text, None, false, Some(card.id.clone()), None)?;
    record(app, TaskEventKind::Moved { card: card.id.clone(), column: TaskColumn::Doing, by: "you".into() });
    Ok(pane)
}

/// The log of the card `card`, else of the agent `name` from `pane_id`'s
/// tab, else of that tab — what its board's History shows — or all of it
/// with `all`.
pub fn history(
    state: &AppState,
    name: Option<String>,
    all: bool,
    pane_id: Option<String>,
    card: Option<String>,
) -> Result<Vec<TaskHistoryEntry>, String> {
    let name = match name {
        Some(name) if card.is_none() && !all => name,
        _ => {
            let tab = if card.is_some() || all { None } else { Some(tab_of(state, pane_id)?) };
            return lines(state, card, tab);
        }
    };
    let events = events(state);
    let pane = crate::handoff::find(state, &name, pane_id)?;
    let tab = tab_of(state, Some(pane.clone())).ok();
    let sender = crate::handoff::sender(state, &pane);
    let scope = cmux_core::tasks::Scope::Agent { pane: &pane, name: sender.as_deref(), tab: tab.as_deref() };
    Ok(cmux_core::tasks::history(&events, scope))
}

/// The history a board pane shows, as `agent history` gives it: of the
/// card `card`, else of the tab `tab_id`, or of every tab when both are
/// absent. Oldest first.
#[tauri::command(async)]
pub fn task_history(
    state: State<'_, AppState>,
    tab_id: Option<String>,
    card: Option<String>,
) -> Result<Vec<TaskHistoryEntry>, String> {
    lines(&state, card, tab_id)
}

/// The log of the card `card`, else of the tab `tab`, else all of it.
fn lines(state: &AppState, card: Option<String>, tab: Option<String>) -> Result<Vec<TaskHistoryEntry>, String> {
    let events = events(state);
    match (card, tab) {
        (Some(card), _) => cmux_core::tasks::card_history(&events, &card),
        (None, Some(tab)) => Ok(cmux_core::tasks::history(&events, cmux_core::tasks::Scope::Tab(&tab))),
        (None, None) => Ok(cmux_core::tasks::history(&events, cmux_core::tasks::Scope::All)),
    }
}

/// The cards a board pane shows, as `task list` does: of the tab
/// `tab_id`, or of every tab when absent.
#[tauri::command(async)]
pub fn task_cards(state: State<'_, AppState>, tab_id: Option<String>) -> Vec<TaskCard> {
    board(&state, tab_id)
}

