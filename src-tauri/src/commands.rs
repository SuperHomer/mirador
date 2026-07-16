use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State};

use cmux_core::osc::{OscEvent, OscScanner};
use cmux_protocol::{Direction, NotificationDto, SplitDir, WorkspaceSnapshot};

use crate::{notify, AppState};

#[derive(Clone, Serialize)]
struct PaneExitPayload {
    pane_id: String,
}

fn build_snapshot(state: &AppState) -> WorkspaceSnapshot {
    let mut snapshot = {
        let ws = state.workspace.lock().unwrap();
        let meta = state.meta.lock().unwrap();
        ws.snapshot(&meta)
    };
    notify::decorate_snapshot(state, &mut snapshot);
    snapshot
}

pub fn emit_workspace(app: &AppHandle) {
    let state = app.state::<AppState>();
    let snapshot = build_snapshot(&state);
    let _ = app.emit("workspace-changed", snapshot);
}

#[tauri::command]
pub fn workspace_snapshot(state: State<'_, AppState>) -> WorkspaceSnapshot {
    build_snapshot(&state)
}

#[tauri::command]
pub fn list_notifications(state: State<'_, AppState>) -> Vec<NotificationDto> {
    state.notifications.lock().unwrap().clone()
}

#[tauri::command]
pub fn mark_all_notifications_read(app: AppHandle, state: State<'_, AppState>) {
    if notify::mark_all_read(&state) {
        emit_workspace(&app);
    }
}

/// Frontend answers a `read-screen-request` round-trip from the socket.
#[tauri::command]
pub fn resolve_screen_read(state: State<'_, AppState>, request_id: u64, text: String) {
    if let Some(tx) = state.screen_requests.lock().unwrap().remove(&request_id) {
        let _ = tx.send(text);
    }
}

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> cmux_protocol::ResolvedConfig {
    state.config.lock().unwrap().clone()
}

/// `command`, when set, is typed into the new pane's shell once it spawns.
#[tauri::command]
pub fn new_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    command: Option<String>,
) -> String {
    let (tab_id, pane) = state.workspace.lock().unwrap().new_tab();
    if let Some(cmd) = command {
        state.pending_commands.lock().unwrap().insert(pane, cmd);
    }
    emit_workspace(&app);
    tab_id
}

#[tauri::command]
pub fn close_tab(app: AppHandle, state: State<'_, AppState>, tab_id: String) {
    let killed = state.workspace.lock().unwrap().close_tab(&tab_id);
    kill_panes(&state, &killed);
    emit_workspace(&app);
}

#[tauri::command]
pub fn set_active_tab(app: AppHandle, state: State<'_, AppState>, tab_id: String) {
    let focused = {
        let mut ws = state.workspace.lock().unwrap();
        ws.set_active_tab(&tab_id);
        ws.focused_pane()
    };
    notify::mark_pane_read(&state, &focused);
    emit_workspace(&app);
}

#[tauri::command]
pub fn rename_tab(app: AppHandle, state: State<'_, AppState>, tab_id: String, title: String) {
    state.workspace.lock().unwrap().rename_tab(&tab_id, &title);
    emit_workspace(&app);
}

#[tauri::command]
pub fn move_tab(app: AppHandle, state: State<'_, AppState>, tab_id: String, to: usize) {
    state.workspace.lock().unwrap().move_tab(&tab_id, to);
    emit_workspace(&app);
}

/// Splits `pane_id`, adding a fresh (not yet attached) pane to the tree.
/// The frontend mounts a TerminalPane for it and calls `attach_pane`.
#[tauri::command]
pub fn split_pane(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    dir: SplitDir,
    command: Option<String>,
) -> Result<String, String> {
    let new_pane = state
        .workspace
        .lock()
        .unwrap()
        .split_pane(&pane_id, dir)
        .ok_or_else(|| format!("no pane {pane_id}"))?;
    // Inherit the source pane's cwd so the new shell opens there.
    {
        let mut meta = state.meta.lock().unwrap();
        if let Some(m) = meta.get(&pane_id).cloned() {
            meta.insert(new_pane.clone(), m);
        }
    }
    if let Some(cmd) = command {
        state
            .pending_commands
            .lock()
            .unwrap()
            .insert(new_pane.clone(), cmd);
    }
    emit_workspace(&app);
    Ok(new_pane)
}

#[tauri::command]
pub fn close_pane(app: AppHandle, state: State<'_, AppState>, pane_id: String) {
    let killed = state.workspace.lock().unwrap().close_pane(&pane_id);
    kill_panes(&state, &killed);
    emit_workspace(&app);
}

#[tauri::command]
pub fn focus_pane(app: AppHandle, state: State<'_, AppState>, pane_id: String) {
    let focused = state.workspace.lock().unwrap().focus_pane(&pane_id);
    let read_changed = notify::mark_pane_read(&state, &pane_id);
    if focused || read_changed {
        emit_workspace(&app);
    }
}

#[tauri::command]
pub fn focus_direction(app: AppHandle, state: State<'_, AppState>, direction: Direction) {
    let next = state.workspace.lock().unwrap().focus_direction(direction);
    if let Some(pane) = next {
        notify::mark_pane_read(&state, &pane);
        emit_workspace(&app);
    }
}

#[tauri::command]
pub fn set_split_ratios(
    app: AppHandle,
    state: State<'_, AppState>,
    tab_id: String,
    path: Vec<usize>,
    ratios: Vec<f32>,
) {
    if state
        .workspace
        .lock()
        .unwrap()
        .set_split_ratios(&tab_id, &path, ratios)
    {
        emit_workspace(&app);
    }
}

/// Connects a mounted frontend pane to its PTY: spawns the shell on first
/// attach (or after exit), or just swaps the output channel on remount.
/// Returns true when a fresh shell was spawned.
#[tauri::command]
pub fn attach_pane(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    cols: u16,
    rows: u16,
    on_data: Channel<InvokeResponseBody>,
) -> Result<bool, String> {
    // A queued command (custom command / `cmux run`) can't be typed right
    // at spawn: shell init (zsh + conda etc.) resets the tty and flushes
    // queued input. It stays in AppState.pending_commands until the shell's
    // first output proves init is done — the command survives sink swaps
    // from remounts (React StrictMode mounts panes twice in dev).
    let sink = {
        let sink_app = app.clone();
        let pty = state.pty.clone();
        let pane = pane_id.clone();
        move |bytes: &[u8]| {
            let _ = on_data.send(InvokeResponseBody::Raw(bytes.to_vec()));
            let pending = {
                let state = sink_app.state::<AppState>();
                let mut map = state.pending_commands.lock().unwrap();
                map.remove(&pane)
            };
            if let Some(cmd) = pending {
                let pty = pty.clone();
                let pane = pane.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    let _ = pty.write(&pane, format!("{cmd}\n").as_bytes());
                });
            }
        }
    };

    if state.pty.is_running(&pane_id) {
        state.pty.set_sink(&pane_id, sink);
        let _ = state.pty.resize(&pane_id, cols, rows);
        return Ok(false);
    }

    let cwd = state
        .meta
        .lock()
        .unwrap()
        .get(&pane_id)
        .and_then(|m| m.cwd.clone());

    // Scanner events run on the pane's forwarder thread.
    let scanner = {
        let app = app.clone();
        let pane_id = pane_id.clone();
        OscScanner::new(move |event| match event {
            OscEvent::Notification { title, body } => {
                notify::handle_notification(&app, &pane_id, title, body);
            }
            OscEvent::Cwd(path) => {
                let state = app.state::<AppState>();
                let changed = {
                    let mut meta = state.meta.lock().unwrap();
                    let entry = meta.entry(pane_id.clone()).or_default();
                    if entry.cwd.as_deref() != Some(path.as_str()) {
                        entry.cwd = Some(path);
                        true
                    } else {
                        false
                    }
                };
                if changed {
                    emit_workspace(&app);
                }
            }
            OscEvent::Title(title) => {
                let state = app.state::<AppState>();
                let changed = {
                    let mut meta = state.meta.lock().unwrap();
                    let entry = meta.entry(pane_id.clone()).or_default();
                    let title = if title.trim().is_empty() {
                        None
                    } else {
                        Some(title)
                    };
                    if entry.title != title {
                        entry.title = title;
                        true
                    } else {
                        false
                    }
                };
                if changed {
                    emit_workspace(&app);
                }
            }
        })
    };

    let exit_app = app.clone();
    state.pty.spawn(
        &pane_id,
        cols,
        rows,
        cwd.as_deref(),
        None,
        Box::new(scanner),
        sink,
        move |exited_id| {
            let _ = exit_app.emit(
                "pane-exit",
                PaneExitPayload {
                    pane_id: exited_id.to_string(),
                },
            );
        },
    )?;

    Ok(true)
}

#[tauri::command]
pub fn write_pty(state: State<'_, AppState>, pane_id: String, data: String) -> Result<(), String> {
    state.pty.write(&pane_id, data.as_bytes())
}

#[tauri::command]
pub fn resize_pty(
    state: State<'_, AppState>,
    pane_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state.pty.resize(&pane_id, cols, rows)
}

/// Frontend acknowledges processed output bytes; resumes a reader paused at
/// the flow-control high watermark.
#[tauri::command]
pub fn ack_pty(state: State<'_, AppState>, pane_id: String, bytes: u64) {
    state.pty.ack(&pane_id, bytes);
}

fn kill_panes(state: &State<'_, AppState>, panes: &[String]) {
    let mut meta = state.meta.lock().unwrap();
    for pane in panes {
        let _ = state.pty.close(pane);
        meta.remove(pane);
    }
    drop(meta);
    notify::forget_panes(state, panes);
}
