//! Handoffs in the app: finding a teammate by name in the sender's tab,
//! and typing a message into its terminal. The rules — what may be typed
//! into, and how — are `cmux_core::handoff`'s.

use cmux_core::handoff::{Candidate, Readiness};

use crate::AppState;

/// Waits, once the checks that refuse at once have passed, for the agent
/// in `pane` while it is starting — launched, resumed or restored, its
/// terminal running but its command not yet run, so that what is typed
/// would reach its shell — for no longer than the app waits for any agent
/// (`cmux_core::agents::START_GRACE`). Never asleep holding the lock. An
/// agent forgotten meanwhile is one the app gave up on.
fn wait_started(state: &AppState, pane: &str) -> Result<(), String> {
    loop {
        let readiness = {
            let meta = state.meta.lock().unwrap();
            let m = meta.get(pane);
            cmux_core::handoff::readiness(
                m.is_some_and(|m| m.is_agent()),
                m.is_some_and(|m| m.agent_live),
                m.and_then(|m| m.agent_started_at).map(|t| t.elapsed()),
                cmux_core::agents::AUTO_RESUME,
            )
        };
        match readiness {
            Readiness::Ready => return Ok(()),
            Readiness::Starting => std::thread::sleep(std::time::Duration::from_millis(250)),
            Readiness::NeverStarted | Readiness::NotAgent => {
                return Err(format!("its agent has not started yet (pane {pane})"))
            }
        }
    }
}

/// The row a menu on `pane`'s screen has selected, when its agent shows
/// one (`cmux_core::handoff::choice_on_screen`). A screen that cannot be
/// read — its terminal not mounted, the read timed out — counts as no
/// menu: refusing then would refuse every send to a tab never shown.
fn choice_shown(app: &tauri::AppHandle, pane: &str) -> Option<String> {
    let screen = crate::server::read_screen(app, pane, None).ok()?;
    cmux_core::handoff::choice_on_screen(&screen).map(String::from)
}

/// The agents of `caller`'s tab, by every name they go by.
fn candidates(state: &AppState, caller: &str) -> Result<Vec<Candidate>, String> {
    let ws = state.workspace.lock().unwrap();
    let tab = ws.tab_of_pane(caller).ok_or_else(|| format!("no pane {caller}"))?;
    let meta = state.meta.lock().unwrap();
    Ok(cmux_core::layout::pane_ids(&tab.root)
        .into_iter()
        .filter_map(|pane| {
            let m = meta.get(&pane).filter(|m| m.is_agent())?;
            let member = tab
                .team
                .as_ref()
                .and_then(|t| t.members.iter().find(|s| s.pane == pane))
                .map(|s| s.name.clone());
            Some(Candidate { member, session: m.agent_name.clone(), role: m.agent_role.clone(), pane })
        })
        .collect())
}

/// The pane `name` means from `caller`'s tab: a teammate by name, or any
/// pane by its id.
pub fn find(state: &AppState, name: &str, caller: Option<String>) -> Result<String, String> {
    if state.workspace.lock().unwrap().all_pane_ids().iter().any(|p| p == name) {
        return Ok(name.to_string());
    }
    let caller = caller.unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    cmux_core::handoff::resolve(name, &candidates(state, &caller)?)
}

/// Types `message` into `to`'s terminal, said to be from the caller when
/// the caller is an agent, and logs it on `card` — a card written by hand
/// — when given one, else on a card of its own titled `title` (or from the
/// message). Returns the recipient's pane.
pub fn send(
    app: &tauri::AppHandle,
    to: &str,
    message: &str,
    caller: Option<String>,
    force: bool,
    card: Option<String>,
    title: Option<String>,
) -> Result<String, String> {
    use tauri::Manager;
    let state = &*app.state::<AppState>();
    let card = card.map(|id| crate::tasks::written(state, &id)).transpose()?;
    let pane = find(state, to, caller.clone())?;
    if caller.as_deref() == Some(pane.as_str()) {
        return Err("that is the pane this runs in: an agent cannot message itself".into());
    }
    let status = |state: &AppState| {
        let meta = state.meta.lock().unwrap();
        let m = meta
            .get(&pane)
            .filter(|m| m.is_agent())
            .ok_or_else(|| format!("{to} runs no agent: a message would run in its shell as a command"))?;
        Ok::<_, String>(m.status())
    };
    cmux_core::handoff::can_receive(status(state)?, force).map_err(|e| format!("{to}: {e}"))?;
    if !state.pty.is_running(&pane) {
        return Err(format!("{to}'s terminal has not started yet: show its tab once, then send again"));
    }
    wait_started(state, &pane).map_err(|e| format!("{to}: {e}"))?;
    // As it is once up, not as it was before the wait: still an agent, and
    // perhaps already asking a person something.
    cmux_core::handoff::can_receive(status(state)?, force).map_err(|e| format!("{to}: {e}"))?;
    // A dialog no hook reports, which the Enter below would answer. Not
    // read when forced: the answer would not change, and a read can take
    // seconds.
    if !force {
        if let Some(row) = choice_shown(app, &pane) {
            return Err(format!("{to} is showing a choice, `{row}`: answer it in its pane, or send with --force"));
        }
    }
    let from = caller.as_deref().and_then(|c| sender(state, c));
    let (paste, enter) = cmux_core::handoff::keystrokes(message, from.as_deref())?;
    state.pty.write(&pane, &paste)?;
    // Enter on its own read: a TUI that gets it with the paste can take it
    // as part of the paste and leave the message unsent.
    std::thread::sleep(std::time::Duration::from_millis(120));
    state.pty.write(&pane, &enter)?;
    let from = from.unwrap_or_else(|| "you".into());
    crate::tasks::given(app, cmux_protocol::TaskSource::AgentSend, from, &pane, message, card, title);
    Ok(pane)
}

/// Starts the agent in `pane` on a new conversation, by typing its
/// harness's command for that. Refused, with what `team clear` reports
/// and why, for a pane running no agent, one whose terminal has not
/// started, one waiting on a person or showing a menu unless `force`, and
/// a harness with no such command. A working agent is not refused: the
/// line waits for its turn to end, which is how a lead clears itself last.
pub fn clear_pane(app: &tauri::AppHandle, pane: &str, force: bool) -> Result<(), (&'static str, String)> {
    use tauri::Manager;
    let state = &*app.state::<AppState>();
    let agent = |state: &AppState| {
        let meta = state.meta.lock().unwrap();
        let m = meta.get(pane).filter(|m| m.is_agent()).ok_or(("not running", "it runs no agent".to_string()))?;
        Ok((m.status(), m.harness()))
    };
    agent(state)?;
    if !state.pty.is_running(pane) {
        return Err(("not running", "its terminal has not started yet: show its tab once, then clear again".into()));
    }
    wait_started(state, pane).map_err(|e| ("not running", e))?;
    // As it is once up, not as it was before the wait.
    let (status, harness) = agent(state)?;
    cmux_core::handoff::can_receive(status, force).map_err(|e| ("needs you", e))?;
    if !force {
        if let Some(row) = choice_shown(app, pane) {
            return Err(("needs you", format!("it is showing a choice, `{row}`: answer it in its pane, or clear with --force")));
        }
    }
    let line = cmux_core::handoff::clear_line(harness).map_err(|e| ("no harness", e))?;
    state.pty.write(pane, line.as_bytes()).map_err(|e| ("not running", e))?;
    // Enter on its own read, as `send` does.
    std::thread::sleep(std::time::Duration::from_millis(120));
    state.pty.write(pane, b"\r").map_err(|e| ("not running", e))?;
    Ok(())
}

/// `clear_pane` for the agent `name` means from the caller's tab, as
/// `send` finds it, or the caller itself. Returns its pane.
pub fn clear(app: &tauri::AppHandle, name: Option<&str>, caller: Option<String>, force: bool) -> Result<String, String> {
    use tauri::Manager;
    let state = &*app.state::<AppState>();
    let pane = match name {
        Some(name) => find(state, name, caller)?,
        None => caller.unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane()),
    };
    let label = name.unwrap_or(&pane).to_string();
    clear_pane(app, &pane, force).map_err(|(_, why)| format!("{label}: {why}"))?;
    Ok(pane)
}

/// What the agent in `pane` calls itself in a message; none when it is no
/// agent — a person, then.
pub fn sender(state: &AppState, pane: &str) -> Option<String> {
    candidates(state, pane)
        .ok()?
        .into_iter()
        .find(|x| x.pane == pane)
        .and_then(|x| cmux_core::handoff::sender_name(&x))
}
