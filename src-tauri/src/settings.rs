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

/// Zooms every terminal `steps` whole points, or back to the default with
/// 0, through `fontSize`. The new size: the file is reloaded at once, the
/// watcher's way (`config_watch::reload`), so the next step starts from it.
#[tauri::command]
pub fn zoom_terminals(app: AppHandle, state: State<'_, AppState>, steps: i32) -> Result<f32, String> {
    let current = state.config.lock().unwrap().font_size;
    cmux_core::config::zoom::zoom_terminals(current, steps)?;
    Ok(crate::config_watch::reload(&app)?.font_size)
}

/// Shows the computers pane if one is open, in whatever tab: the
/// computers are in mirador.json, one file, so a second pane would only
/// show them twice. Otherwise opens one, a split of `pane_id` (the
/// focused pane when absent) or a tab of its own.
#[tauri::command]
pub fn open_computers(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: Option<String>,
    tab: bool,
) -> Result<String, String> {
    if let Some(pane) = show_existing_pane(&app, &state, |m| m.computers) {
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
    state.meta.lock().unwrap().entry(pane.clone()).or_default().computers = true;
    emit_workspace(&app);
    Ok(pane)
}

/// mirador.json as it is now. One that does not parse is the answer, not
/// the defaults it would load as, where no computer is saved.
pub(crate) fn saved_config() -> Result<cmux_core::config::Config, String> {
    match cmux_core::config::load() {
        (_, Some(problem)) => Err(problem),
        (config, None) => Ok(config),
    }
}

/// The usable computers mirador.json lists, for the palette and the
/// computers pane; or why the file cannot be read, rather than none.
#[tauri::command]
pub fn list_computers() -> Result<Vec<cmux_core::config::Computer>, String> {
    Ok(cmux_core::config::computers_in(&saved_config()?))
}

/// Adds a computer to mirador.json; refused for a name that is taken or
/// not shell-safe, or an empty host or directory.
#[tauri::command]
pub fn add_computer(name: String, host: String, dir: String) -> Result<cmux_core::config::Computer, String> {
    cmux_core::config::settings::add_computer(&name, &host, &dir)
}

/// Changes the computer `original` in mirador.json; refused as an added
/// one is, or for a name another computer has.
#[tauri::command]
pub fn update_computer(original: String, name: String, host: String, dir: String) -> Result<cmux_core::config::Computer, String> {
    cmux_core::config::settings::update_computer(&original, &name, &host, &dir)
}

/// Removes the computer `name` from mirador.json.
#[tauri::command]
pub fn remove_computer(name: String) -> Result<(), String> {
    cmux_core::config::settings::remove_computer(&name)
}

/// Tests `host` and `dir` as a computer for agents, saved or not: the
/// pane tests what its form says before it is saved. Up to
/// `ssh::CHECK_TIMEOUT`, so off the main thread, as `settings_view`.
#[tauri::command]
pub async fn test_computer(host: String, dir: String) -> Result<cmux_protocol::ComputerCheck, String> {
    #[cfg(windows)]
    return {
        let _ = (host, dir);
        Err("testing a computer is not done on Windows yet".into())
    };
    #[cfg(not(windows))]
    tauri::async_runtime::spawn_blocking(move || cmux_core::ssh::check_computer(&host, &dir, env!("CARGO_PKG_VERSION")))
        .await
        .map_err(|e| e.to_string())?
}
