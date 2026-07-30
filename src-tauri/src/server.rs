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
use cmux_protocol::{Request, RequestEnvelope, ResponseEnvelope};

use crate::{notify, AppState};

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let path = cmux_core::ipc::default_socket_path();

        // Another Mirador owning automation is an ordinary situation (a
        // second window, a dev build beside the installed app), not a
        // failure — say so plainly instead of leaking an errno. Binding
        // still refuses to steal the endpoint if one appears in between.
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

fn focused_pane(state: &AppState) -> String {
    state.workspace.lock().unwrap().focused_pane()
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
        Request::FocusPane { pane_id } => {
            crate::commands::focus_pane(app.clone(), app.state(), pane_id);
            Ok(Value::Null)
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
        } => {
            let pane = resolve_pane(&state, pane_id, tty).unwrap_or_else(|| focused_pane(&state));
            notify::handle_notification(app, &pane, title, body);
            Ok(json!({ "paneId": pane }))
        }
        Request::AgentSession {
            pane_id,
            tty,
            agent,
            session_id,
        } => {
            let pane = resolve_pane(&state, pane_id, tty)
                .ok_or("could not resolve the pane for this agent session")?;
            {
                let mut meta = state.meta.lock().unwrap();
                meta.entry(pane.clone()).or_default().agent_session =
                    Some(format!("{agent}:{session_id}"));
            }
            state
                .session_dirty
                .store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(json!({ "paneId": pane }))
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
            Ok(json!({ "paneId": pane_id }))
        }
        Request::BrowserNavigate { pane_id, url } => {
            let pane = resolve_browser_pane(&state, pane_id)?;
            crate::browser::navigate(app, &pane, &url)?;
            Ok(json!({ "paneId": pane }))
        }
        Request::BrowserSnapshot { pane_id } => {
            let pane = resolve_browser_pane(&state, pane_id)?;
            let result = crate::browser::execute(app, &pane, json!({ "op": "snapshot" }))?;
            parse_bridge_result(&pane, &result)
        }
        Request::BrowserClick { pane_id, target } => {
            let pane = resolve_browser_pane(&state, pane_id)?;
            let result =
                crate::browser::execute(app, &pane, json!({ "op": "click", "target": target }))?;
            parse_bridge_result(&pane, &result)
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
            parse_bridge_result(&pane, &result)
        }
        Request::BrowserEval { pane_id, js } => {
            let pane = resolve_browser_pane(&state, pane_id)?;
            let result = crate::browser::execute(app, &pane, json!({ "op": "eval", "js": js }))?;
            parse_bridge_result(&pane, &result)
        }
        Request::BrowserHistory { pane_id, action } => {
            let pane = resolve_browser_pane(&state, pane_id)?;
            crate::browser::history(app, &pane, &action)?;
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

/// Which pane a request came from: the `MIRA_PANE` the caller inherited
/// (checked against live panes, since a stale environment outlives its
/// pane), else its tty.
fn resolve_pane(state: &AppState, pane_id: Option<String>, tty: Option<String>) -> Option<String> {
    let claimed = pane_id.filter(|id| {
        let ws = state.workspace.lock().unwrap();
        ws.tabs
            .iter()
            .any(|t| cmux_core::layout::contains(&t.root, id))
    });
    claimed.or_else(|| tty.as_deref().and_then(|t| pane_by_tty(state, t)))
}

/// Finds the pane whose shell owns the given tty (e.g. "ttys004"). The
/// primary way a process identifies its pane is the `MIRA_PANE` environment
/// variable it inherited; this is the fallback for processes that lost the
/// environment, and Windows (no ttys) has only the former.
#[cfg(unix)]
fn pane_by_tty(state: &AppState, tty: &str) -> Option<String> {
    let wanted = tty.trim_start_matches("/dev/");
    for (pane, pid) in state.pty.pids() {
        let output = std::process::Command::new("ps")
            .args(["-o", "tty=", "-p", &pid.to_string()])
            .output()
            .ok()?;
        let pane_tty = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if pane_tty == wanted {
            return Some(pane);
        }
    }
    None
}

#[cfg(not(unix))]
fn pane_by_tty(_state: &AppState, _tty: &str) -> Option<String> {
    None
}

/// Default browser pane: the given one, or the first browser pane of the
/// active tab, or any browser pane.
fn resolve_browser_pane(state: &AppState, pane_id: Option<String>) -> Result<String, String> {
    if let Some(id) = pane_id {
        return Ok(id);
    }
    let meta = state.meta.lock().unwrap();
    let browser_panes: Vec<String> = meta
        .iter()
        .filter(|(_, m)| m.browser_url.is_some())
        .map(|(id, _)| id.clone())
        .collect();
    drop(meta);
    if browser_panes.is_empty() {
        return Err("no browser pane (open one with `mira browser open <url>`)".into());
    }
    let ws = state.workspace.lock().unwrap();
    let active_panes = cmux_core::layout::pane_ids(&ws.active_tab().root);
    Ok(browser_panes
        .iter()
        .find(|p| active_panes.contains(p))
        .unwrap_or(&browser_panes[0])
        .clone())
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
