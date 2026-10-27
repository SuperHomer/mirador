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
/// file cannot be read.
#[tauri::command]
pub fn settings_view() -> Result<Vec<SettingRow>, String> {
    cmux_core::config::settings::settings_view()
}
