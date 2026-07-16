mod commands;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use cmux_core::pty::PtyManager;
use cmux_core::state::{PaneMeta, Workspace};
use tauri::Manager;

pub struct AppState {
    pub pty: PtyManager,
    pub workspace: Mutex<Workspace>,
    pub meta: Mutex<HashMap<String, PaneMeta>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            pty: PtyManager::new(),
            workspace: Mutex::new(Workspace::default()),
            meta: Mutex::new(HashMap::new()),
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            setup_menu(app.handle())?;
            spawn_cwd_poller(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::workspace_snapshot,
            commands::new_tab,
            commands::close_tab,
            commands::set_active_tab,
            commands::rename_tab,
            commands::move_tab,
            commands::split_pane,
            commands::close_pane,
            commands::focus_pane,
            commands::focus_direction,
            commands::set_split_ratios,
            commands::attach_pane,
            commands::write_pty,
            commands::resize_pty,
            commands::ack_pty,
        ])
        .run(tauri::generate_context!())
        .expect("error while running cmux");
}

/// Minimal app menu: keeps Edit roles (so Cmd+C/V reach the webview's
/// clipboard machinery) but omits File>Close Window — Cmd+W must reach the
/// keymap to close a *pane*, not the window.
#[cfg(target_os = "macos")]
fn setup_menu(handle: &tauri::AppHandle) -> tauri::Result<()> {
    use tauri::menu::{AboutMetadata, Menu, PredefinedMenuItem, Submenu};

    let app_menu = Submenu::with_items(
        handle,
        "cmux",
        true,
        &[
            &PredefinedMenuItem::about(handle, None, Some(AboutMetadata::default()))?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::hide(handle, None)?,
            &PredefinedMenuItem::hide_others(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::quit(handle, None)?,
        ],
    )?;
    let edit_menu = Submenu::with_items(
        handle,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(handle, None)?,
            &PredefinedMenuItem::redo(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::cut(handle, None)?,
            &PredefinedMenuItem::copy(handle, None)?,
            &PredefinedMenuItem::paste(handle, None)?,
            &PredefinedMenuItem::select_all(handle, None)?,
        ],
    )?;
    let menu = Menu::with_items(handle, &[&app_menu, &edit_menu])?;
    handle.set_menu(menu)?;
    Ok(())
}

/// Polls each pane's shell cwd (proc-based fallback until OSC 7 lands in
/// M4) and pushes a fresh snapshot when anything changed.
fn spawn_cwd_poller(handle: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(2));
        let state = handle.state::<AppState>();
        let mut changed = false;
        {
            let pids = state.pty.pids();
            let mut meta = state.meta.lock().unwrap();
            for (pane, pid) in pids {
                if let Some(cwd) = cmux_core::cwd::process_cwd(pid) {
                    let entry = meta.entry(pane).or_default();
                    if entry.cwd.as_deref() != Some(cwd.as_str()) {
                        entry.cwd = Some(cwd);
                        changed = true;
                    }
                }
            }
        }
        if changed {
            commands::emit_workspace(&handle);
        }
    });
}
