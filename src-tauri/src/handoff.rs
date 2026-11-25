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
                // No process on a remote is ever seen from here, as none is
                // on Windows.
                cmux_core::agents::AUTO_RESUME && !m.is_some_and(|m| m.remote_host.is_some()),
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

/// Waits for the terminal of a remote agent's pane opened moments ago —
/// by `agent new`, `team add` or a restart — to attach, for no longer than
/// the app waits for any agent to start (`cmux_core::agents::START_GRACE`).
/// The agent is started over ssh before the app shows its pane, so its
/// hooks can report it running while the pane's own `mira attach` is still
/// to come: a message sent then was refused for a pane about to be back.
/// One between attaches for any other reason — reconnecting, restored and
/// waiting for a key, done — is not waited for: nothing says it is coming.
/// Never asleep holding the lock.
fn wait_attached(state: &AppState, pane: &str) {
    while !state.pty.is_running(pane) {
        let restored = state.restored_panes.lock().unwrap().contains(pane);
        let opening = state.meta.lock().unwrap().get(pane).is_some_and(|m| {
            m.remote_command.is_some()
                && m.remote_host.is_some()
                && m.agent_started_at.is_some_and(|t| t.elapsed() < cmux_core::agents::START_GRACE)
        });
        if restored || !opening {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// How long a Claude Code or Codex agent, once running, is given to show
/// its input box. It draws one within a second or two of starting, and
/// `wait_started` has waited for it to start already; a screen still
/// without one is on something of its own (a welcome, a login) that
/// waiting does not change, so the caller hears so in seconds, not in the
/// 20 a start is given.
const INPUT_BOX_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// Why what is typed must not go into `pane` now, by what its screen shows
/// (`cmux_core::handoff::showing_in`): a menu, which the Enter after it would
/// answer; or, for a harness whose input box Mirador knows, no input box
/// within [`INPUT_BOX_WAIT`] — the paste might dismiss a welcome screen
/// and the Enter land on the menu that replaces it. No read outlives that
/// wait: each is given only the time left. A screen that cannot be read at
/// all — its terminal not mounted, the read timed out — counts as ready:
/// refusing then would refuse every send to a tab never shown.
fn screen_refusal(app: &tauri::AppHandle, pane: &str, harness: Option<cmux_protocol::Harness>) -> Option<String> {
    use cmux_core::handoff::Showing;
    use cmux_protocol::Harness;
    use std::time::{Duration, Instant};
    let knows_input_box = matches!(harness, Some(Harness::Claude | Harness::Codex | Harness::Pi | Harness::OpenCode));
    let no_input_box =
        || Some("shows no input box yet: it may still be starting, or waiting on a screen of its own — look at its pane".into());
    let deadline = Instant::now() + INPUT_BOX_WAIT;
    // Once a screen with neither was read, a read cut short by the
    // deadline is the wait running out, not an unreadable screen.
    let mut seen_neither = false;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if seen_neither && left.is_zero() {
            return no_input_box();
        }
        let screen = match crate::server::read_screen(app, pane, None, left.min(Duration::from_secs(5))) {
            Ok(screen) => screen,
            Err(_) if seen_neither => return no_input_box(),
            Err(_) => return None,
        };
        match cmux_core::handoff::showing_in(&screen, harness) {
            Showing::Choice(row) => return Some(format!("is showing a choice, `{row}`: answer it in its pane")),
            Showing::Composer => return None,
            Showing::Neither if !knows_input_box => return None,
            Showing::Neither => {
                seen_neither = true;
                std::thread::sleep(left.min(Duration::from_millis(250)));
            }
        }
    }
}

/// The agents of `caller`'s tab, by every name they go by.
fn candidates(state: &AppState, caller: &str) -> Result<Vec<Candidate>, String> {
    let ws = state.workspace.lock().unwrap();
    let tab = ws.tab_of_pane(caller).ok_or_else(|| format!("no pane {caller}"))?;
    Ok(agents_of(tab, &state.meta.lock().unwrap()))
}

/// The agents of the tab running the team called `team`: none when no
/// tab runs one, and an error when two do, since a name could then mean
/// either.
fn team_candidates(state: &AppState, team: &str) -> Result<Option<Vec<Candidate>>, String> {
    let ws = state.workspace.lock().unwrap();
    let tabs: Vec<&cmux_core::state::Tab> =
        ws.tabs.iter().filter(|t| t.team.as_ref().is_some_and(|run| run.name == team)).collect();
    match tabs.as_slice() {
        [] => Ok(None),
        [tab] => Ok(Some(agents_of(tab, &state.meta.lock().unwrap()))),
        many => {
            let ids: Vec<&str> = many.iter().map(|t| t.id.as_str()).collect();
            Err(format!("team `{team}` is running in more than one tab ({}): name the pane by its id", ids.join(", ")))
        }
    }
}

/// The names of the teams running now, for an error that names a team
/// that is not one of them.
fn running_teams(state: &AppState) -> String {
    let ws = state.workspace.lock().unwrap();
    let names: Vec<&str> = ws.tabs.iter().filter_map(|t| Some(t.team.as_ref()?.name.as_str())).collect();
    if names.is_empty() {
        " — there are none".into()
    } else {
        format!(" (there are: {})", names.join(", "))
    }
}

fn agents_of(tab: &cmux_core::state::Tab, meta: &std::collections::HashMap<String, cmux_core::state::PaneMeta>) -> Vec<Candidate> {
    cmux_core::layout::pane_ids(&tab.root)
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
        .collect()
}

/// The pane `name` means from `caller`'s tab: a teammate by name, the
/// agent `<name>` of the tab running team `<team>` as `<team>/<name>`, or
/// any pane by its id.
pub fn find(state: &AppState, name: &str, caller: Option<String>) -> Result<String, String> {
    if state.workspace.lock().unwrap().all_pane_ids().iter().any(|p| p == name) {
        return Ok(name.to_string());
    }
    let caller = caller.unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    let here = || cmux_core::handoff::resolve(name, &candidates(state, &caller)?, None);
    let Some((team, member)) = name.trim().split_once('/') else {
        return here();
    };
    let team = team.trim();
    match team_candidates(state, team)? {
        Some(there) => cmux_core::handoff::resolve(member, &there, Some(team)),
        // A session's name is free text and may hold a `/` of its own:
        // what reached a teammate before still does.
        None => here().map_err(|_| format!("no team `{team}` is running{}", running_teams(state))),
    }
}

/// Types `message` into `to`'s terminal, said to be from the caller when
/// the caller is an agent, and logs it on `card` — a card written by hand
/// — when given one, else on a card of its own titled `title` (or from the
/// message). Returns the recipient's pane, and whether the message was
/// queued instead (`Queued`).
///
/// Unless `force`, a message waits in the pane's queue, to be typed by
/// [`deliver`], when the person is in the way there — a draft in the input
/// box, or keys typed within `cmux_core::handoff::TYPING_QUIET` — or other
/// messages already wait for it: pasted then, it would be merged into the
/// person's draft and sent with it (#244).
pub fn send(
    app: &tauri::AppHandle,
    to: &str,
    message: &str,
    caller: Option<String>,
    force: bool,
    card: Option<String>,
    title: Option<String>,
) -> Result<(String, Option<Queued>), String> {
    use tauri::Manager;
    let state = &*app.state::<AppState>();
    let card = card.map(|id| crate::tasks::written(state, &id).map(|c| c.id)).transpose()?;
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
        Ok::<_, String>((m.status(), m.harness()))
    };
    cmux_core::handoff::can_receive(status(state)?.0, force).map_err(|e| format!("{to}: {e}"))?;
    wait_attached(state, &pane);
    if !state.pty.is_running(&pane) {
        // A remote agent's pane with no terminal is between attaches:
        // reconnecting, waiting for a key, or done. The app cannot tell.
        let remote = state.meta.lock().unwrap().get(&pane).filter(|m| m.remote_command.is_some()).and_then(|m| m.remote_host.clone());
        return Err(match remote {
            Some(host) => format!("{to} on {host} is not attached right now: send again once its pane is back"),
            None => format!("{to}'s terminal has not started yet: show its tab once, then send again"),
        });
    }
    wait_started(state, &pane).map_err(|e| format!("{to}: {e}"))?;
    // As it is once up, not as it was before the wait: still an agent, and
    // perhaps already asking a person something.
    let (status, harness) = status(state)?;
    cmux_core::handoff::can_receive(status, force).map_err(|e| format!("{to}: {e}"))?;
    // Not read when forced: the answer would not change, and a read can
    // take seconds.
    if !force {
        if let Some(why) = screen_refusal(app, &pane, harness) {
            return Err(format!("{to} {why}, or send with --force"));
        }
    }
    let to_tab = state.workspace.lock().unwrap().tab_of_pane(&pane).map(|t| t.id.clone());
    let from = caller.as_deref().and_then(|c| sender_in(state, c, to_tab.as_deref()));
    let (paste, enter) = cmux_core::handoff::keystrokes(message, from.as_deref())?;
    if !force {
        let (draft, typed) = person_in(app, state, &pane, harness);
        let mut queues = state.queued.lock().unwrap();
        let ahead = queues.get(&pane).map_or(0, |q| q.len());
        if let Some(why) = cmux_core::handoff::queue_reason(draft, typed, ahead) {
            let id = state.next_queued.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let queued = Queued { id, ahead, why };
            let message = Message { id, message: message.to_string(), from, card, title, paste, enter };
            let queue = queues.entry(pane.clone()).or_default();
            queue.push_back(message);
            // One deliverer a pane, for as long as its queue is not empty:
            // it removes the queue, under this lock, when it ends.
            if queue.len() == 1 {
                let (app, pane) = (app.clone(), pane.clone());
                std::thread::spawn(move || deliver(app, pane));
            }
            drop(queues);
            // Its pane shows how many wait (`AgentInfo::queued`).
            crate::commands::broadcast_workspace(app);
            return Ok((pane, Some(queued)));
        }
    }
    type_message(app, state, &pane, harness, &paste, &enter)?;
    let from = from.unwrap_or_else(|| "you".into());
    crate::tasks::given(app, cmux_protocol::TaskSource::AgentSend, from, &pane, message, card, title);
    Ok((pane, None))
}

/// A message `send` queued: its id, how many were ahead of it, and why it
/// waits.
pub struct Queued {
    pub id: u64,
    pub ahead: usize,
    pub why: String,
}

/// A message waiting in a pane's queue, with what `send` logs it with.
pub struct Message {
    id: u64,
    message: String,
    from: Option<String>,
    card: Option<String>,
    title: Option<String>,
    paste: Vec<u8>,
    enter: Vec<u8>,
}

/// Whether the person is in `pane`'s way: a draft in its agent's input
/// box (`draft`), and keys typed into it within `TYPING_QUIET`. A screen
/// that cannot be read has no draft, as `screen_refusal` counts it ready.
fn person_in(app: &tauri::AppHandle, state: &AppState, pane: &str, harness: Option<cmux_protocol::Harness>) -> (bool, bool) {
    let draft = crate::server::read_screen_at(app, pane, None, std::time::Duration::from_secs(2))
        .is_ok_and(|(screen, cursor)| draft(state, pane, &screen, cursor, harness));
    (draft, typed_lately(state, pane))
}

/// Whether `screen` shows a person's draft in `pane`'s input box
/// (`cmux_core::handoff::has_draft`, by the cursor) — only in a pane
/// someone has typed into. A draft comes from typing, and the check rests
/// on where Claude Code and Codex park their cursor: should one park it
/// elsewhere, every box would read as a draft and no message would ever
/// go in. Never typed into, a pane has no person's draft, whatever its
/// cursor says. The limit of that: `typed_at` is in memory, so after a
/// relaunch a draft typed before the quit, its pane's process kept
/// (`persistSessions`), is not seen, and a message would be merged into
/// it — until the person types there again.
fn draft(state: &AppState, pane: &str, screen: &str, cursor: Option<(usize, usize)>, harness: Option<cmux_protocol::Harness>) -> bool {
    let typed_in = state.typed_at.lock().unwrap().contains_key(pane);
    typed_in && cursor.is_some_and(|at| cmux_core::handoff::has_draft(screen, at, harness))
}

/// Whether the person typed into `pane` within `TYPING_QUIET`.
fn typed_lately(state: &AppState, pane: &str) -> bool {
    state.typed_at.lock().unwrap().get(pane).is_some_and(|t| t.elapsed() < cmux_core::handoff::TYPING_QUIET)
}

/// Types the messages queued for `pane`, oldest first, each once the
/// agent is ready for it and the person is not in the way: about every
/// 500 ms it looks, and types the head of the queue when the agent is
/// neither working nor waiting on a person, its screen shows its input
/// box, empty, and nothing was typed there within `TYPING_QUIET`. Not
/// while it works: the person's own prompt, just sent, is that turn, and a
/// message typed into it would not be a prompt of its own — nor would
/// `send --wait` tell its turn from theirs. One message a look, so the
/// next waits for the box to be empty again. Ends with the queue: emptied,
/// or dropped once the pane is closed or runs no agent. No lock is held
/// across a read or a sleep.
fn deliver(app: tauri::AppHandle, pane: String) {
    use cmux_protocol::AgentStatus;
    use tauri::Manager;
    let state = &*app.state::<AppState>();
    let _dropped_on_panic = DropQueueOnPanic { state, pane: &pane };
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let open = state.workspace.lock().unwrap().all_pane_ids().contains(&pane);
        let agent = state.meta.lock().unwrap().get(&pane).filter(|m| m.is_agent()).map(|m| (m.status(), m.harness()));
        let Some((status, harness)) = agent.filter(|_| open) else {
            state.queued.lock().unwrap().remove(&pane);
            crate::commands::broadcast_workspace(&app);
            return;
        };
        if matches!(status, Some(AgentStatus::Working | AgentStatus::NeedsYou)) || !state.pty.is_running(&pane) {
            continue;
        }
        let Ok((screen, cursor)) = crate::server::read_screen_at(&app, &pane, None, std::time::Duration::from_secs(2)) else {
            continue;
        };
        if cmux_core::handoff::showing_in(&screen, harness) != cmux_core::handoff::Showing::Composer {
            continue;
        }
        if draft(state, &pane, &screen, cursor, harness) || typed_lately(state, &pane) {
            continue;
        }
        let head = {
            let queues = state.queued.lock().unwrap();
            queues.get(&pane).and_then(|q| q.front()).map(|m| (m.id, m.paste.clone(), m.enter.clone()))
        };
        let Some((id, paste, enter)) = head else { return };
        if type_message(&app, state, &pane, harness, &paste, &enter).is_err() {
            continue;
        }
        state.delivered.lock().unwrap().insert(id, crate::runs::now_ms());
        let (message, emptied) = {
            let mut queues = state.queued.lock().unwrap();
            let Some(queue) = queues.get_mut(&pane) else { return };
            let message = queue.pop_front();
            let emptied = queue.is_empty();
            if emptied {
                queues.remove(&pane);
            }
            (message, emptied)
        };
        crate::commands::broadcast_workspace(&app);
        if let Some(m) = message {
            let from = m.from.unwrap_or_else(|| "you".into());
            crate::tasks::given(&app, cmux_protocol::TaskSource::AgentSend, from, &pane, &m.message, m.card, m.title);
        }
        if emptied {
            return;
        }
    }
}

/// Drops `pane`'s queue if its deliverer panics. Left behind, a queue no
/// deliverer works would hold every later message for the pane, since a
/// send spawns one only for a queue it starts; dropped, `send --wait`
/// says so as for a closed pane, and the next send starts afresh. Only on
/// a panic: a deliverer that ends normally has removed its queue itself,
/// and by the time this ran a new send could own a new one.
struct DropQueueOnPanic<'a> {
    state: &'a AppState,
    pane: &'a str,
}

impl Drop for DropQueueOnPanic<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.state.queued.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(self.pane);
        }
    }
}

/// Where a message `send` queued is: still queued, `ahead` of it; typed,
/// at `delivered` (ms since the epoch); or neither — dropped with its
/// pane's queue.
pub fn queued(state: &AppState, id: u64) -> (Option<usize>, Option<u64>) {
    let ahead = state.queued.lock().unwrap().values().find_map(|q| q.iter().position(|m| m.id == id));
    (ahead, state.delivered.lock().unwrap().get(&id).copied())
}

/// Types a message into `pane`: `paste`, then `enter` on a write of its
/// own once the paste has landed, the person's keys held in between
/// (`Typing`).
fn type_message(
    app: &tauri::AppHandle,
    state: &AppState,
    pane: &str,
    harness: Option<cmux_protocol::Harness>,
    paste: &[u8],
    enter: &[u8],
) -> Result<(), String> {
    // Held from before the screen is read: Codex's read can take seconds,
    // and a key pressed then would land under the paste.
    let typing = Typing::start(state, pane);
    let before = (harness == Some(cmux_protocol::Harness::Codex))
        .then(|| crate::server::read_screen(app, pane, None, std::time::Duration::from_secs(2)).ok())
        .flatten();
    state.pty.write(pane, paste)?;
    if let Some(before) = before {
        // Characters, not bytes: Codex handles one key per character.
        let chars = std::str::from_utf8(paste).map_or(paste.len(), |s| s.chars().count());
        wait_pasted(app, pane, before, chars);
    }
    // Enter on its own read: a TUI that gets it with the paste can take it
    // as part of the paste and leave the message unsent.
    std::thread::sleep(cmux_core::handoff::enter_delay(harness));
    state.pty.write(pane, enter)?;
    drop(typing);
    Ok(())
}

/// Holds the person's keystrokes to `pane` while a message is typed into
/// it (`cmux_core::handoff::KeyHold`), and types them once it is dropped —
/// on every way out, an error's included: a hold left behind would swallow
/// the person's keyboard.
struct Typing<'a> {
    state: &'a AppState,
    pane: String,
}

impl<'a> Typing<'a> {
    fn start(state: &'a AppState, pane: &str) -> Self {
        state.key_hold.lock().unwrap().hold(pane);
        Typing { state, pane: pane.to_string() }
    }
}

impl Drop for Typing<'_> {
    fn drop(&mut self) {
        // Written under the lock, so a key typed meanwhile waits behind them.
        let mut hold = self.state.key_hold.lock().unwrap_or_else(|e| e.into_inner());
        let kept = hold.release(&self.pane);
        if !kept.is_empty() {
            let _ = self.state.pty.write(&self.pane, &kept);
        }
    }
}

/// Reads `pane`'s screen until the paste of `chars` characters written
/// after `before` was read shows there (`cmux_core::handoff::PasteWatch`,
/// which says why). A screen that cannot be read, or a Codex that never
/// shows the paste, must not hang the send, so this returns anyway once
/// the read fails or the budget runs out, and the Enter goes as it would
/// have. The budget grows with the paste (`cmux_core::handoff::paste_wait`,
/// beside the measurement it comes from).
fn wait_pasted(app: &tauri::AppHandle, pane: &str, before: String, chars: usize) {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + cmux_core::handoff::paste_wait(chars);
    let mut watch = cmux_core::handoff::PasteWatch::new(before);
    loop {
        std::thread::sleep(Duration::from_millis(150));
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return;
        }
        let Ok(screen) = crate::server::read_screen(app, pane, None, left) else { return };
        if watch.landed(&screen) {
            return;
        }
    }
}

/// Starts the agent in `pane` on a new conversation, by typing its
/// harness's command for that. Refused, with what `team clear` reports
/// and why, for a pane running no agent, one whose terminal has not
/// started, one waiting on a person, showing a menu or no input box unless
/// `force` (`screen_refusal`), and a harness with no such command. A working agent is not refused: the
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
    wait_attached(state, pane);
    if !state.pty.is_running(pane) {
        // As for a send: a remote agent's pane between attaches.
        let remote = state.meta.lock().unwrap().get(pane).filter(|m| m.remote_command.is_some()).and_then(|m| m.remote_host.clone());
        return Err((
            "not running",
            match remote {
                Some(host) => format!("it is not attached on {host} right now: clear again once its pane is back"),
                None => "its terminal has not started yet: show its tab once, then clear again".into(),
            },
        ));
    }
    wait_started(state, pane).map_err(|e| ("not running", e))?;
    // As it is once up, not as it was before the wait.
    let (status, harness) = agent(state)?;
    cmux_core::handoff::can_receive(status, force).map_err(|e| ("needs you", e))?;
    if !force {
        if let Some(why) = screen_refusal(app, pane, harness) {
            return Err(("needs you", format!("it {why}, or clear with --force")));
        }
        let (draft, typed) = person_in(app, state, pane, harness);
        if let Some(why) = cmux_core::handoff::person_busy(draft, typed) {
            return Err(("needs you", format!("{why}, or clear with --force")));
        }
    }
    let line = cmux_core::handoff::clear_line(harness).map_err(|e| ("no harness", e))?;
    let typing = Typing::start(state, pane);
    state.pty.write(pane, line.as_bytes()).map_err(|e| ("not running", e))?;
    // Enter on its own read, as `send` does.
    std::thread::sleep(cmux_core::handoff::enter_delay(harness));
    state.pty.write(pane, b"\r").map_err(|e| ("not running", e))?;
    drop(typing);
    // A Pi's or an OpenCode's `/new` moves it to a conversation of its own
    // making, whose id is not cheap to learn: Pi writes the new session's
    // file only on its first message, in a directory every Pi in that
    // project shares, and OpenCode keeps its ids in its server's store. The
    // one Mirador started it on is the cleared one, and a restart must not
    // bring that back: the record goes, and the pane comes back with
    // nothing to resume. Only this clear is known: a `/new` or a session
    // switch typed in the pane itself leaves the record on the old
    // conversation, which a restart or a save then brings back.
    if matches!(harness, Some(cmux_protocol::Harness::Pi | cmux_protocol::Harness::OpenCode)) {
        if let Some(m) = state.meta.lock().unwrap().get_mut(pane) {
            m.agent_session = None;
        }
        state.session_dirty.store(true, std::sync::atomic::Ordering::Relaxed);
    }
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

/// [`sender`], as an agent in tab `seen_from` can reply to it: its name
/// from its own tab; from another, `<team>/<name>` when its tab runs a
/// team only it runs, else its pane id — a bare name would find nobody
/// there, or someone else. None when it is no agent.
pub fn sender_in(state: &AppState, pane: &str, seen_from: Option<&str>) -> Option<String> {
    let name = sender(state, pane);
    let ws = state.workspace.lock().unwrap();
    let tab = ws.tab_of_pane(pane);
    if seen_from.is_none() || tab.map(|t| t.id.as_str()) == seen_from {
        return name;
    }
    if !state.meta.lock().unwrap().get(pane).is_some_and(|m| m.is_agent()) {
        return None;
    }
    let team = tab.and_then(|t| t.team.as_ref()).map(|run| run.name.as_str());
    let only = |team: &str| ws.tabs.iter().filter(|t| t.team.as_ref().is_some_and(|r| r.name == team)).count() == 1;
    Some(match (team, name) {
        (Some(team), Some(name)) if only(team) => format!("{team}/{name}"),
        _ => pane.to_string(),
    })
}
