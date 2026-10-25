//! Automation socket server: newline-delimited JSON over a Unix domain
//! socket (macOS/Linux) or a named pipe (Windows). Every verb goes through
//! the same code paths as the UI commands, so the CLI and the UI can never
//! diverge in behavior.

use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use cmux_core::transport::{self, Stream};
use cmux_protocol::{Request, RequestEnvelope, ResponseEnvelope, WallView};

use crate::{notify, AppState};

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let path = cmux_core::ipc::default_socket_path();

        // Another Mirador owning automation is an ordinary situation (a
        // dev build beside the installed app), not a failure — say so
        // plainly instead of leaking an errno. Binding still refuses to
        // steal the endpoint if one appears in between.
        if transport::is_live(&path) {
            eprintln!("mirador: automation socket already served by another instance");
            return;
        }

        let listener = match transport::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("mirador: automation socket unavailable: {e}");
                return;
            }
        };
        if let Err(e) = cmux_core::ipc::write_discovery(&path) {
            eprintln!("mirador: could not write socket discovery file: {e}");
        }

        loop {
            let Ok(stream) = listener.accept() else {
                // A listener that can no longer produce connections would
                // otherwise spin this thread at 100% CPU.
                std::thread::sleep(Duration::from_millis(200));
                continue;
            };
            let app = app.clone();
            std::thread::spawn(move || serve_connection(app, stream));
        }
    });
}

/// One client, one request stream. The reader owns the connection and
/// lends it back for each response — a duplex stream can't be split.
fn serve_connection(app: AppHandle, stream: Box<dyn Stream>) {
    let mut reader = BufReader::new(stream);

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<RequestEnvelope>(&line) {
            Ok(envelope) => {
                let id = envelope.id;
                match dispatch(&app, envelope.req) {
                    Ok(data) => ResponseEnvelope {
                        id,
                        ok: true,
                        data: Some(data),
                        error: None,
                    },
                    Err(error) => ResponseEnvelope {
                        id,
                        ok: false,
                        data: None,
                        error: Some(error),
                    },
                }
            }
            Err(e) => ResponseEnvelope {
                id: None,
                ok: false,
                data: None,
                error: Some(format!("bad request: {e}")),
            },
        };
        let Ok(mut bytes) = serde_json::to_vec(&response) else {
            break;
        };
        bytes.push(b'\n');
        let stream = reader.get_mut();
        if stream.write_all(&bytes).is_err() || stream.flush().is_err() {
            break;
        }
    }
}

/// Who a request from `pane_id` comes from, for the task log: the agent
/// there, or else a person.
fn sender(state: &AppState, pane_id: Option<&str>) -> String {
    pane_id.and_then(|p| crate::handoff::sender(state, p)).unwrap_or_else(|| "you".into())
}

fn focused_pane(state: &AppState) -> String {
    state.workspace.lock().unwrap().focused_pane()
}

/// The pane whose trail is asked for: the agent `name` as `agent send`
/// finds it from `pane_id`'s tab, else `pane_id`, else the focused pane.
fn trail_pane(state: &AppState, name: Option<String>, pane_id: Option<String>) -> Result<String, String> {
    match name {
        Some(name) => crate::handoff::find(state, &name, pane_id),
        None => Ok(pane_id.unwrap_or_else(|| focused_pane(state))),
    }
}

fn dispatch(app: &AppHandle, req: Request) -> Result<Value, String> {
    let state = app.state::<AppState>();
    match req {
        Request::ListTabs => {
            let snapshot = crate::commands::workspace_snapshot(app.state());
            serde_json::to_value(snapshot).map_err(|e| e.to_string())
        }
        Request::NewTab { command } => {
            let result = crate::commands::new_tab(app.clone(), app.state(), command);
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        Request::SplitPane {
            pane_id,
            dir,
            command,
        } => {
            let pane = pane_id.unwrap_or_else(|| focused_pane(&state));
            let new_pane =
                crate::commands::split_pane(app.clone(), app.state(), pane, dir, command)?;
            Ok(json!({ "paneId": new_pane }))
        }
        Request::ClosePane { pane_id } => {
            crate::commands::close_pane(app.clone(), app.state(), pane_id);
            Ok(Value::Null)
        }
        Request::CloseTab { tab_id } => {
            crate::commands::close_tab(app.clone(), app.state(), tab_id);
            Ok(Value::Null)
        }
        Request::FocusPane { pane_id } => {
            crate::commands::focus_pane(app.clone(), app.state(), pane_id.clone());
            // Asked for from outside, so on the board its card is brought
            // into view even when it already had the focus — the snapshot
            // alone says only that the focus changed, if it did.
            let _ = app.emit("focus-requested", pane_id);
            Ok(Value::Null)
        }
        Request::ZoomPane { pane_id, zoom } => {
            let pane = pane_id.unwrap_or_else(|| focused_pane(&state));
            let zoomed =
                crate::commands::zoom_pane(app.clone(), app.state(), pane.clone(), zoom)?;
            Ok(json!({ "paneId": pane, "zoomed": zoomed }))
        }
        Request::View { view } => {
            if let Some(view) = view {
                crate::commands::set_view(app.clone(), app.state(), view);
            }
            let view = state.workspace.lock().unwrap().view;
            Ok(json!({ "view": view }))
        }
        Request::SetCamera { x, y, zoom, width, height } => {
            crate::commands::set_camera(app.state(), x, y, zoom, width, height)?;
            Ok(Value::Null)
        }
        Request::SetCardRect { pane_id, x, y, w, h } => {
            let placed = crate::commands::set_card_rect(app.clone(), app.state(), pane_id, x, y, w, h)?;
            serde_json::to_value(placed).map_err(|e| e.to_string())
        }
        Request::SetFramePosition { tab_id, x, y } => {
            let placed = crate::commands::set_frame_position(app.clone(), app.state(), tab_id, x, y)?;
            serde_json::to_value(placed).map_err(|e| e.to_string())
        }
        Request::SendInput { pane_id, data } => {
            let pane = pane_id.unwrap_or_else(|| focused_pane(&state));
            state.pty.write(&pane, data.as_bytes())?;
            Ok(Value::Null)
        }
        Request::ReadScreen { pane_id, lines } => {
            let pane = pane_id.unwrap_or_else(|| focused_pane(&state));
            let text = read_screen(app, &pane, lines)?;
            Ok(json!({ "paneId": pane, "text": text }))
        }
        Request::Notify {
            pane_id,
            tty,
            title,
            body,
            job,
        } => {
            let pane =
                resolve_pane(&state, pane_id, tty, job).unwrap_or_else(|| focused_pane(&state));
            notify::handle_notification(app, &pane, title, body);
            Ok(json!({ "paneId": pane }))
        }
        Request::Quit { end_sessions } => {
            // Answer first: once the app exits, nobody is left to reply.
            let app = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if end_sessions {
                    tauri::async_runtime::block_on(crate::commands::quit_ending_sessions(app));
                } else {
                    app.exit(0);
                }
            });
            Ok(Value::Null)
        }
        Request::AgentSession {
            pane_id,
            tty,
            agent,
            session_id,
            job,
            event,
            notification_type,
            message,
            transcript_path,
        } => {
            if agent != "claude" {
                return Err(format!("unknown agent `{agent}`"));
            }
            let record = cmux_core::agents::session_record(&session_id, job.as_deref())
                .ok_or("not a session id")?;
            let status = event.as_deref().and_then(|e| {
                cmux_core::agents::hook_status(e, notification_type.as_deref(), message.as_deref())
            });
            // `PostToolUse` fires on every tool call of every session, and
            // matters only to a pane that is waiting on the human: with none
            // waiting, answer before resolving anything (that takes a `ps`).
            if event.as_deref() == Some("post-tool") {
                let waiting = state.meta.lock().unwrap().values().any(|m| {
                    m.agent_status == Some(cmux_protocol::AgentStatus::NeedsYou)
                });
                if !waiting {
                    return Ok(Value::Null);
                }
            }
            let pane = resolve_pane(&state, pane_id, tty, job)
                .ok_or("could not resolve the pane for this agent session")?;
            let turn = {
                let mut meta = state.meta.lock().unwrap();
                let entry = meta.entry(pane.clone()).or_default();
                let before = entry.status();
                entry.agent_session = Some(record);
                if let Some(path) = transcript_path
                    .map(std::path::PathBuf::from)
                    .filter(|p| cmux_core::transcript::is_transcript_of(p, &session_id))
                {
                    entry.agent_transcript = Some(path);
                }
                if let Some(status) = status {
                    if entry.agent_status != Some(status) {
                        entry.agent_status = Some(status);
                        entry.agent_since_ms = Some(crate::runs::now_ms());
                    }
                    // A message describes what the agent is blocked on; once
                    // it moves on, the message is stale.
                    entry.agent_message = match status {
                        cmux_protocol::AgentStatus::NeedsYou => message,
                        _ => None,
                    };
                }
                cmux_core::tasks::status_event(&pane, before, entry.status())
            };
            if let Some(turn) = turn {
                crate::tasks::record(app, turn);
            }
            state
                .session_dirty
                .store(true, std::sync::atomic::Ordering::Relaxed);
            crate::commands::request_workspace_emit(app);
            Ok(json!({ "paneId": pane }))
        }
        Request::AgentNew {
            role,
            task,
            target,
            pane_id,
        } => {
            let from = sender(&state, pane_id.as_deref());
            let pane_id = crate::commands::open_agent_from(
                app.clone(),
                app.state(),
                role,
                task,
                pane_id,
                // Beside the pane it was asked from, in the tab you work in;
                // a tab of its own only when asked for one.
                target.as_deref() == Some("tab"),
                from,
            )?;
            Ok(json!({ "paneId": pane_id }))
        }
        Request::AgentList => {
            let agents = crate::commands::agent_infos(&state);
            Ok(json!({ "agents": agents }))
        }
        Request::AgentActivity { pane_id } => {
            let pane = pane_id.unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
            let activity = crate::commands::agent_activity(&state, &pane)?;
            Ok(serde_json::to_value(activity).map_err(|e| e.to_string())?)
        }
        Request::AgentEditor { pane_id, target } => {
            let pane = crate::agent_editor::open_agent_editor(
                app.clone(),
                app.state(),
                pane_id,
                target.as_deref() == Some("tab"),
            )?;
            Ok(json!({ "paneId": pane }))
        }
        Request::AgentSend { to, message, pane_id, force, card } => {
            let pane = crate::handoff::send(app, &to, &message, pane_id, force, card)?;
            Ok(json!({ "paneId": pane }))
        }
        Request::AgentFind { name, pane_id } => {
            let pane = crate::handoff::find(&state, &name, pane_id)?;
            Ok(json!({ "paneId": pane }))
        }
        Request::TeamStart { team, task, pane_id } => {
            let from = sender(&state, pane_id.as_deref());
            let tab = crate::teams::start_team_from(app.clone(), app.state(), team, task, pane_id, from)?;
            Ok(json!({ "tabId": tab }))
        }
        Request::TeamList => Ok(json!({ "teams": crate::teams::team_list(&state) })),
        Request::TaskList { all, pane_id } => Ok(json!({ "cards": crate::tasks::list(&state, all, pane_id)? })),
        Request::TaskAdd { title, to, pane_id } => {
            let from = sender(&state, pane_id.as_deref());
            Ok(json!({ "cardId": crate::tasks::add(app, &title, to, pane_id, from)? }))
        }
        Request::TaskMove { card, column, pane_id } => {
            let by = sender(&state, pane_id.as_deref());
            crate::tasks::move_card(app, &card, column, by)?;
            Ok(Value::Null)
        }
        Request::AgentTrail { name, pane_id, turns } => {
            let pane = trail_pane(&state, name, pane_id)?;
            Ok(json!({ "trail": crate::commands::agent_trail(&state, &pane, turns, None) }))
        }
        Request::AgentTrailDetail { action_id, name, pane_id } => {
            let pane = trail_pane(&state, name, pane_id)?;
            Ok(json!({ "action": crate::commands::trail_detail(&state, &pane, &action_id)? }))
        }
        Request::AgentHistory { name, all, pane_id, card } => {
            Ok(json!({ "history": crate::tasks::history(&state, name, all, pane_id, card)? }))
        }
        Request::TeamAdd { role, name, pane_id } => {
            let pane = crate::teams::team_add(app.clone(), app.state(), role, name, pane_id)?;
            Ok(json!({ "paneId": pane }))
        }
        Request::TeamRestart { members, pane_id } => {
            let done = crate::teams::team_restart(app.clone(), app.state(), members, pane_id)?;
            Ok(json!({ "members": done }))
        }
        Request::AgentBoard { pane_id } => {
            crate::commands::set_wall_view(app.clone(), app.state(), pane_id, WallView::Tasks)?;
            Ok(json!({ "wall": true, "wallView": WallView::Tasks }))
        }
        Request::AgentWall { pane_id } => {
            let on = crate::commands::toggle_agent_wall(app.clone(), app.state(), pane_id)?;
            Ok(json!({ "wall": on }))
        }
        Request::Run {
            command,
            target,
            wait,
            timeout_secs,
        } => {
            let pane_id = match target.as_deref() {
                Some("tab") => {
                    crate::commands::new_tab(app.clone(), app.state(), Some(command)).pane_id
                }
                _ => {
                    let pane = focused_pane(&state);
                    crate::commands::split_pane(
                        app.clone(),
                        app.state(),
                        pane,
                        cmux_protocol::SplitDir::Row,
                        Some(command),
                    )?
                }
            };
            if !wait {
                return Ok(json!({ "paneId": pane_id }));
            }
            // Register before the frontend can possibly attach + finish.
            let rx = crate::runs::register_waiter(&state, &pane_id);
            let exit_code =
                crate::runs::wait_for_exit(&state, rx, &pane_id, timeout_secs.unwrap_or(600))?;
            let output = crate::runs::take_capture(&state, &pane_id);
            Ok(json!({ "paneId": pane_id, "exitCode": exit_code, "output": output }))
        }
        Request::ListRuns => {
            let runs = crate::runs::list(&state);
            Ok(json!({ "runs": runs }))
        }
        Request::BrowserOpen { url, target } => {
            let pane_id = crate::commands::open_browser(
                app.clone(),
                app.state(),
                None,
                target.as_deref() == Some("tab"),
                url,
            )?;
            crate::browser::wait_for_first_load(app, &pane_id)?;
            state.browser_bridge.used(&pane_id);
            Ok(json!({ "paneId": pane_id }))
        }
        Request::GraphOpen { target, pane_id } => {
            let pane_id = crate::commands::open_graph(
                app.clone(),
                app.state(),
                pane_id,
                target.as_deref() == Some("tab"),
            )?;
            Ok(json!({ "paneId": pane_id }))
        }
        Request::DiffWorktrees { pane_id } => {
            let pane = pane_id
                .filter(|id| state.meta.lock().unwrap().contains_key(id))
                .unwrap_or_else(|| focused_pane(&state));
            let worktrees = crate::commands::worktrees_for_source(&state, &pane)?;
            serde_json::to_value(worktrees).map_err(|e| e.to_string())
        }
        Request::DiffOpen {
            spec,
            target,
            pane_id,
            worktree,
        } => {
            let pane_id = crate::commands::open_diff(
                app.clone(),
                app.state(),
                pane_id,
                target.as_deref() == Some("tab"),
                spec,
                worktree,
            )?;
            Ok(json!({ "paneId": pane_id }))
        }
        Request::BrowserNavigate { pane_id, url } => {
            let pane = resolve_browser_pane(&state, pane_id)?;
            let note = crate::browser::navigate(app, &pane, &url)?
                .and_then(|before| crate::browser::wait_for_new_page(app, &pane, before));
            Ok(json!({ "paneId": pane, "note": note }))
        }
        Request::BrowserSnapshot { pane_id } => {
            let pane = resolve_browser_pane(&state, pane_id)?;
            let result = crate::browser::execute(app, &pane, json!({ "op": "snapshot" }))?;
            parse_bridge_result(&pane, &result)
        }
        Request::BrowserScreenshot { pane_id } => {
            use base64::Engine;
            let pane = resolve_browser_pane(&state, pane_id)?;
            let png = crate::browser::screenshot(app, &pane)?;
            Ok(json!({ "paneId": pane, "png": base64::engine::general_purpose::STANDARD.encode(png) }))
        }
        Request::BrowserClick { pane_id, target } => {
            let pane = resolve_browser_pane(&state, pane_id)?;
            let result =
                crate::browser::execute(app, &pane, json!({ "op": "click", "target": target }))?;
            parse_bridge_result(&pane, &result)?;
            Ok(Value::Null)
        }
        Request::BrowserFill {
            pane_id,
            target,
            value,
        } => {
            let pane = resolve_browser_pane(&state, pane_id)?;
            let result = crate::browser::execute(
                app,
                &pane,
                json!({ "op": "fill", "target": target, "value": value }),
            )?;
            parse_bridge_result(&pane, &result)?;
            Ok(Value::Null)
        }
        Request::BrowserEval { pane_id, js } => {
            let pane = resolve_browser_pane(&state, pane_id)?;
            let result = crate::browser::execute(app, &pane, json!({ "op": "eval", "js": js }))?;
            parse_bridge_result(&pane, &result)
        }
        Request::BrowserHistory { pane_id, action } => {
            let pane = resolve_browser_pane(&state, pane_id)?;
            let before = crate::browser::history(app, &pane, &action)?;
            // Back, forward and reload wait for the page they go to, as
            // navigate does.
            match crate::browser::wait_for_new_page(app, &pane, before) {
                Some(note) => Ok(json!({ "note": note })),
                None => Ok(Value::Null),
            }
        }
        Request::BrowserText { pane_id, selector } => {
            let pane = resolve_browser_pane(&state, pane_id)?;
            let result = crate::browser::execute(app, &pane, json!({ "op": "text", "selector": selector }))?;
            parse_bridge_result(&pane, &result)
        }
        Request::BrowserType { pane_id, target, text } => {
            let pane = resolve_browser_pane(&state, pane_id)?;
            let result =
                crate::browser::execute(app, &pane, json!({ "op": "type", "target": target, "text": text }))?;
            parse_bridge_result(&pane, &result)?;
            Ok(Value::Null)
        }
        Request::BrowserPress { pane_id, key, target } => {
            let pane = resolve_browser_pane(&state, pane_id)?;
            let result =
                crate::browser::execute(app, &pane, json!({ "op": "press", "key": key, "target": target }))?;
            parse_bridge_result(&pane, &result)?;
            Ok(Value::Null)
        }
        Request::BrowserConsole { pane_id, errors, clear } => {
            let pane = resolve_browser_pane(&state, pane_id)?;
            let result = crate::browser::execute(
                app,
                &pane,
                json!({ "op": "console", "errors": errors, "clear": clear }),
            )?;
            parse_bridge_result(&pane, &result)
        }
        Request::BrowserWait {
            pane_id,
            selector,
            text,
            url,
            gone,
            timeout_secs,
        } => {
            use crate::browser::Until;
            let until = match (selector, text, url, gone) {
                (Some(s), None, None, None) => Until::Shown(s),
                (None, Some(t), None, None) => Until::Text(t),
                (None, None, Some(u), None) => Until::Url(u),
                (None, None, None, Some(g)) => Until::Gone(g),
                _ => return Err("wait for one thing: a selector, --text, --url or --gone".into()),
            };
            let pane = resolve_browser_pane(&state, pane_id)?;
            crate::browser::wait(app, &pane, &until, Duration::from_secs(timeout_secs.unwrap_or(10)))?;
            Ok(Value::Null)
        }
        Request::SshOpen { host, target } => {
            let pane_id = crate::commands::open_ssh(
                app.clone(),
                app.state(),
                None,
                target.as_deref() == Some("tab"),
                host,
            )?;
            Ok(json!({ "paneId": pane_id }))
        }
        Request::SshHosts => Ok(json!({ "hosts": cmux_core::ssh::config_hosts() })),
        Request::SshForward {
            pane_id,
            port,
            cancel,
        } => {
            let (pane, host) = resolve_remote_pane(&state, pane_id)?;
            cmux_core::ssh::forward(&host, port, cancel)?;
            Ok(json!({ "paneId": pane, "port": port, "cancelled": cancel }))
        }
    }
}

/// Default remote pane: the given one, or the active tab's remote pane, or
/// any remote pane. Returns (pane_id, host spec).
fn resolve_remote_pane(
    state: &AppState,
    pane_id: Option<String>,
) -> Result<(String, String), String> {
    let meta = state.meta.lock().unwrap();
    if let Some(id) = pane_id {
        let host = meta
            .get(&id)
            .and_then(|m| m.remote_host.clone())
            .ok_or_else(|| format!("pane {id} is not an SSH pane"))?;
        return Ok((id, host));
    }
    let remotes: Vec<(String, String)> = meta
        .iter()
        .filter_map(|(id, m)| Some((id.clone(), m.remote_host.clone()?)))
        .collect();
    drop(meta);
    if remotes.is_empty() {
        return Err("no SSH pane (open one with `mira ssh open <host>`)".into());
    }
    let ws = state.workspace.lock().unwrap();
    let active_panes = cmux_core::layout::pane_ids(&ws.active_tab().root);
    Ok(remotes
        .iter()
        .find(|(id, _)| active_panes.contains(id))
        .unwrap_or(&remotes[0])
        .clone())
}

/// Which pane a request came from. The `MIRA_PANE` the caller inherited is
/// checked against live panes, since a stale environment outlives its pane,
/// and then yields to the caller's terminal wherever there is one — see
/// `cmux_core::agents::hook_pane` for why an inherited id cannot be trusted.
fn resolve_pane(
    state: &AppState,
    pane_id: Option<String>,
    tty: Option<String>,
    job: Option<String>,
) -> Option<String> {
    let claimed = pane_id.filter(|id| {
        let ws = state.workspace.lock().unwrap();
        ws.tabs
            .iter()
            .any(|t| cmux_core::layout::contains(&t.root, id))
    });
    let procs = if tty.is_some() || job.is_some() {
        cmux_core::agents::process_table()
    } else {
        Vec::new()
    };
    // Windows lists no processes here; a job id there would match nothing
    // and discard the claim, so it only counts where it can be resolved.
    let job = job.filter(|_| cfg!(unix));
    cmux_core::agents::hook_pane(
        &procs,
        &state.pty.pids(),
        claimed,
        tty.as_deref(),
        job.as_deref(),
    )
}

/// The browser pane a command goes to: the given one; else, of the active
/// tab's browser panes — or every tab's, when it has none — the one opened
/// or driven last, or failing that the first in layout order.
fn resolve_browser_pane(state: &AppState, pane_id: Option<String>) -> Result<String, String> {
    let pane = match pane_id {
        Some(id) => id,
        None => {
            let browsers: Vec<String> = {
                // One lock at a time: never meta and workspace together.
                let browser_ids: std::collections::HashSet<String> = state
                    .meta
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(_, m)| m.browser_url.is_some())
                    .map(|(id, _)| id.clone())
                    .collect();
                let ws = state.workspace.lock().unwrap();
                let is_browser = |p: &String| browser_ids.contains(p);
                let active: Vec<String> =
                    cmux_core::layout::pane_ids(&ws.active_tab().root).into_iter().filter(is_browser).collect();
                if active.is_empty() {
                    ws.tabs.iter().flat_map(|t| cmux_core::layout::pane_ids(&t.root)).filter(is_browser).collect()
                } else {
                    active
                }
            };
            state
                .browser_bridge
                .latest_of(&browsers)
                .ok_or("no browser pane (open one with `mira browser open <url>`)")?
        }
    };
    state.browser_bridge.used(&pane);
    Ok(pane)
}

/// Bridge results are JSON; error payloads become protocol errors.
fn parse_bridge_result(pane: &str, raw: &str) -> Result<Value, String> {
    let mut value: Value =
        serde_json::from_str(raw).map_err(|e| format!("bad bridge payload: {e}"))?;
    if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
        return Err(err.to_string());
    }
    if let Some(obj) = value.as_object_mut() {
        obj.insert("paneId".into(), json!(pane));
    }
    Ok(value)
}

/// Round-trips to the webview: the frontend serializes the pane's buffer
/// and resolves the pending request. 5s timeout.
fn read_screen(app: &AppHandle, pane_id: &str, lines: Option<u32>) -> Result<String, String> {
    let state = app.state::<AppState>();
    // The webview's terminal buffer outlives the PTY, so exited command
    // panes and idle restored panes are still readable — that's exactly
    // when an agent wants the final output.
    let known = {
        let ws = state.workspace.lock().unwrap();
        ws.tabs
            .iter()
            .any(|t| cmux_core::layout::contains(&t.root, pane_id))
    };
    if !known {
        return Err(format!("no pane {pane_id}"));
    }
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let request_id = state.next_screen_request.fetch_add(1, Ordering::Relaxed);
    state.screen_requests.lock().unwrap().insert(request_id, tx);

    let _ = app.emit(
        "read-screen-request",
        json!({ "requestId": request_id, "paneId": pane_id, "lines": lines }),
    );

    let result = rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "read-screen timed out (pane not mounted?)".to_string());
    state.screen_requests.lock().unwrap().remove(&request_id);
    result
}
