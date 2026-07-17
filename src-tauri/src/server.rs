//! Automation socket server: newline-delimited JSON over a Unix domain
//! socket. Every verb goes through the same code paths as the UI commands,
//! so the CLI and the UI can never diverge in behavior.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use cmux_protocol::{Request, RequestEnvelope, ResponseEnvelope};

use crate::{notify, AppState};

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let path = cmux_core::ipc::default_socket_path();

        // A stale socket from a crashed instance blocks bind; a live one
        // means another cmux owns automation — leave it alone.
        if path.exists() {
            if UnixStream::connect(&path).is_ok() {
                eprintln!("cmux: automation socket already served by another instance");
                return;
            }
            let _ = std::fs::remove_file(&path);
        }

        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("cmux: automation socket unavailable: {e}");
                return;
            }
        };
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        if let Err(e) = cmux_core::ipc::write_discovery(&path) {
            eprintln!("cmux: could not write socket discovery file: {e}");
        }

        for conn in listener.incoming() {
            let Ok(stream) = conn else { continue };
            let app = app.clone();
            std::thread::spawn(move || serve_connection(app, stream));
        }
    });
}

fn serve_connection(app: AppHandle, stream: UnixStream) {
    let reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut writer = stream;

    for line in reader.lines() {
        let Ok(line) = line else { break };
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
        if writer.write_all(&bytes).is_err() {
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
            title,
            body,
        } => {
            let pane = pane_id.unwrap_or_else(|| focused_pane(&state));
            notify::handle_notification(app, &pane, title, body);
            Ok(Value::Null)
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
    }
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
        return Err("no browser pane (open one with `cmux browser open <url>`)".into());
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
    if !state.pty.is_running(pane_id) {
        return Err(format!("no running pane {pane_id}"));
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
