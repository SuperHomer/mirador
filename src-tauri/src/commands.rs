use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State};

use cmux_core::osc::PassthroughScanner;
use cmux_protocol::{Direction, SplitDir, WorkspaceSnapshot};

use crate::AppState;

#[derive(Clone, Serialize)]
struct PaneExitPayload {
    pane_id: String,
}

pub fn emit_workspace(app: &AppHandle) {
    let state = app.state::<AppState>();
    let snapshot = {
        let ws = state.workspace.lock().unwrap();
        let meta = state.meta.lock().unwrap();
        ws.snapshot(&meta)
    };
    let _ = app.emit("workspace-changed", snapshot);
}

#[tauri::command]
pub fn workspace_snapshot(state: State<'_, AppState>) -> WorkspaceSnapshot {
    let ws = state.workspace.lock().unwrap();
    let meta = state.meta.lock().unwrap();
    ws.snapshot(&meta)
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
    state.workspace.lock().unwrap().set_active_tab(&tab_id);
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
    if state.workspace.lock().unwrap().focus_pane(&pane_id) {
        emit_workspace(&app);
    }
}

#[tauri::command]
pub fn focus_direction(app: AppHandle, state: State<'_, AppState>, direction: Direction) {
    if state
        .workspace
        .lock()
        .unwrap()
        .focus_direction(direction)
        .is_some()
    {
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
    let sink = move |bytes: &[u8]| {
        let _ = on_data.send(InvokeResponseBody::Raw(bytes.to_vec()));
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

    state.pty.spawn(
        &pane_id,
        cols,
        rows,
        cwd.as_deref(),
        None,
        Box::new(PassthroughScanner),
        sink,
        move |exited_id| {
            let _ = app.emit(
                "pane-exit",
                PaneExitPayload {
                    pane_id: exited_id.to_string(),
                },
            );
        },
    )?;

    // Custom command queued for this pane: type it into the fresh shell.
    // The tty input queue buffers it until the shell reads stdin.
    let pending = state.pending_commands.lock().unwrap().remove(&pane_id);
    if let Some(cmd) = pending {
        let _ = state.pty.write(&pane_id, format!("{cmd}\n").as_bytes());
    }
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
}
