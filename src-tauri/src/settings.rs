//! The settings pane: mirador.json's settings, read through
//! `cmux_core::config::settings`, which knows every key.

use tauri::{AppHandle, Emitter, Manager, State};

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
/// file cannot be read.
#[tauri::command]
pub fn settings_view() -> Result<Vec<SettingRow>, String> {
    cmux_core::config::settings::settings_view()
}

/// Sets `key` in mirador.json to `value`, or removes it when null. The
/// pane shows the result when the config watcher sees the file change.
#[tauri::command]
pub fn set_config_value(key: String, value: Option<serde_json::Value>) -> Result<(), String> {
    cmux_core::config::settings::set_config_value(&key, value)
}

/// Zooms every terminal `steps` whole points, or back to the default with
/// 0, through `fontSize`. The new size.
#[tauri::command]
pub fn zoom_terminals(app: AppHandle, state: State<'_, AppState>, steps: i32) -> Result<f32, String> {
    let current = state.config.lock().unwrap().font_size;
    cmux_core::config::zoom::zoom_terminals(current, steps)?;
    Ok(reload_config(&app, &state)?.font_size)
}

/// Zooms the whole window `steps` levels, or back to 100 % with 0,
/// through `uiZoom`. The new zoom.
#[tauri::command]
pub fn zoom_window(app: AppHandle, state: State<'_, AppState>, steps: i32) -> Result<f64, String> {
    let current = state.config.lock().unwrap().ui_zoom;
    cmux_core::config::zoom::zoom_window(current, steps)?;
    Ok(reload_config(&app, &state)?.ui_zoom)
}

/// The config as the file now says, made the app's at once: stored,
/// announced and, for the window's zoom, applied. The watcher would do the
/// same 300 ms after the write, too late for a key held down, whose next
/// step must start from this one; when it does, nothing has changed, so
/// it announces nothing twice.
fn reload_config(app: &AppHandle, state: &AppState) -> Result<cmux_protocol::ResolvedConfig, String> {
    let (cfg, err) = cmux_core::config::load();
    if let Some(err) = err {
        return Err(err);
    }
    let resolved = cmux_core::config::resolve(&cfg);
    *state.config.lock().unwrap() = resolved.clone();
    apply_ui_zoom(app, resolved.ui_zoom);
    let _ = app.emit("config-changed", resolved.clone());
    Ok(resolved)
}

/// Zooms the app's own webview to `zoom` (1 being 100 %): the whole
/// window but its browser panes, which are webviews of their own, at
/// their own zoom.
pub fn apply_ui_zoom(app: &AppHandle, zoom: f64) {
    if let Some(webview) = app.get_webview("main") {
        if let Err(e) = webview.set_zoom(zoom) {
            eprintln!("mirador: could not zoom the window to {zoom}: {e}");
        }
    }
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
