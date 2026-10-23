//! Handoffs in the app: finding a teammate by name in the sender's tab,
//! and typing a message into its terminal. The rules — what may be typed
//! into, and how — are `cmux_core::handoff`'s.

use cmux_core::handoff::Candidate;

use crate::AppState;

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
/// the caller is an agent. Returns the recipient's pane.
pub fn send(state: &AppState, to: &str, message: &str, caller: Option<String>, force: bool) -> Result<String, String> {
    let pane = find(state, to, caller.clone())?;
    if caller.as_deref() == Some(pane.as_str()) {
        return Err("that is the pane this runs in: an agent cannot message itself".into());
    }
    let status = {
        let meta = state.meta.lock().unwrap();
        let m = meta
            .get(&pane)
            .filter(|m| m.is_agent())
            .ok_or_else(|| format!("{to} runs no agent: a message would run in its shell as a command"))?;
        m.status()
    };
    cmux_core::handoff::can_receive(status, force).map_err(|e| format!("{to}: {e}"))?;
    if !state.pty.is_running(&pane) {
        return Err(format!("{to}'s terminal has not started yet: show its tab once, then send again"));
    }
    let from = caller.as_deref().and_then(|c| sender(state, c));
    let (paste, enter) = cmux_core::handoff::keystrokes(message, from.as_deref())?;
    state.pty.write(&pane, &paste)?;
    // Enter on its own read: a TUI that gets it with the paste can take it
    // as part of the paste and leave the message unsent.
    std::thread::sleep(std::time::Duration::from_millis(120));
    state.pty.write(&pane, &enter)?;
    let from = from.unwrap_or_else(|| "you".into());
    crate::tasks::given(state, cmux_protocol::TaskSource::AgentSend, from, &pane, message);
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
