//! The settings pane: mirador.json's settings, read through
//! `cmux_core::config::settings`, which knows every key.

use tauri::{AppHandle, State};

use cmux_protocol::{SettingRow, SplitDir};

use crate::commands::{emit_workspace, show_existing_pane};
use crate::AppState;

/// Shows the settings pane if one is open, in whatever tab: mirador.json
/// is one file, so a second pane would only show it twice. Otherwise
/// opens one, a split of `pane_id` (the focused pane when absent) or a
/// tab of its own.
#[tauri::command]
pub fn open_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: Option<String>,
    tab: bool,
) -> Result<String, String> {
    if let Some(pane) = show_existing_pane(&app, &state, |m| m.settings) {
        return Ok(pane);
    }
    let source = pane_id
        .filter(|id| state.meta.lock().unwrap().contains_key(id))
        .unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    let pane = if tab {
        state.workspace.lock().unwrap().new_tab().1
    } else {
        state
            .workspace
            .lock()
            .unwrap()
            .split_pane(&source, SplitDir::Row)
            .ok_or_else(|| format!("no pane {source}"))?
    };
    state.meta.lock().unwrap().entry(pane.clone()).or_default().settings = true;
    emit_workspace(&app);
    Ok(pane)
}

/// Every setting with the value mirador.json gives it, or the reason the
/// file cannot be read. Off the main thread: on Windows the view looks at
/// each PATH folder for conpty's choices, and an open Settings pane, in
/// any tab, reads it again after every config change, a zoom step
/// included. A PATH folder on a drive that does not answer then slows the
/// pane's refresh, not the app.
#[tauri::command]
pub async fn settings_view() -> Result<Vec<SettingRow>, String> {
    tauri::async_runtime::spawn_blocking(cmux_core::config::settings::settings_view)
        .await
        .map_err(|e| e.to_string())?
}

/// Sets `key` in mirador.json to `value`, or removes it when null. The
/// pane shows the result when the config watcher sees the file change.
#[tauri::command]
pub fn set_config_value(key: String, value: Option<serde_json::Value>) -> Result<(), String> {
    cmux_core::config::settings::set_config_value(&key, value)
}

/// The usable computers mirador.json lists, for the palette.
#[tauri::command]
pub fn list_computers() -> Vec<cmux_core::config::Computer> {
    cmux_core::config::computers_in(&cmux_core::config::load().0)
}

/// Adds a computer to mirador.json; refused for a name that is taken or
/// not shell-safe, or an empty host or directory.
#[tauri::command]
pub fn add_computer(name: String, host: String, dir: String) -> Result<cmux_core::config::Computer, String> {
    cmux_core::config::settings::add_computer(&name, &host, &dir)
}
