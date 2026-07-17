mod commands;
mod config_watch;
mod intel;
mod notify;
mod runs;
#[cfg(unix)]
mod server;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use cmux_core::pty::PtyManager;
use cmux_core::state::{PaneMeta, Workspace};
use cmux_protocol::{NotificationDto, ResolvedConfig, RunRecord};
use tauri::Manager;

pub struct AppState {
    pub pty: PtyManager,
    pub workspace: Mutex<Workspace>,
    pub meta: Mutex<HashMap<String, PaneMeta>>,
    pub config: Mutex<ResolvedConfig>,
    pub notifications: Mutex<Vec<NotificationDto>>,
    pub window_focused: AtomicBool,
    /// Pending read-screen round-trips to the webview.
    pub screen_requests: Mutex<HashMap<u64, std::sync::mpsc::Sender<String>>>,
    pub next_screen_request: AtomicU64,
    /// Command-pane run history (agent audit log).
    pub run_history: Mutex<Vec<RunRecord>>,
    /// Raw output captured per live command pane.
    pub run_captures: Mutex<HashMap<String, Vec<u8>>>,
    /// `run --wait` blockers, resolved on command exit.
    pub run_waiters: Mutex<HashMap<String, Vec<std::sync::mpsc::Sender<Option<i32>>>>>,
    /// (repo_root, branch) → PR status; None = checked, no PR.
    pub pr_cache: Mutex<HashMap<(String, String), Option<cmux_protocol::PrStatus>>>,
    /// Command panes restored from a previous session: they attach idle
    /// (a keypress reruns) instead of auto-running their command.
    pub restored_panes: Mutex<HashSet<String>>,
    /// Set on every workspace change; the saver thread persists and clears.
    pub session_dirty: AtomicBool,
}

pub fn save_session(state: &AppState) {
    let file = {
        let ws = state.workspace.lock().unwrap();
        let meta = state.meta.lock().unwrap();
        cmux_core::session::capture(&ws, &meta)
    };
    if let Err(e) = cmux_core::session::save(&file) {
        eprintln!("cmux: session save failed: {e}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    cmux_core::config::write_default_if_missing();
    let (cfg, cfg_err) = cmux_core::config::load();
    if let Some(err) = &cfg_err {
        eprintln!("cmux: config error, using defaults: {err}");
    }

    // Restore the previous session; command panes come back idle.
    let (workspace, meta) = cmux_core::session::load()
        .and_then(cmux_core::session::restore)
        .unwrap_or_else(|| (Workspace::default(), HashMap::new()));
    let restored_panes: HashSet<String> = meta
        .iter()
        .filter(|(_, m)| m.command.is_some())
        .map(|(id, _)| id.clone())
        .collect();
    cmux_core::session::gc_scrollback(&workspace.all_pane_ids());

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(AppState {
            pty: PtyManager::new(),
            workspace: Mutex::new(workspace),
            meta: Mutex::new(meta),
            config: Mutex::new(cmux_core::config::resolve(&cfg)),
            notifications: Mutex::new(Vec::new()),
            window_focused: AtomicBool::new(true),
            screen_requests: Mutex::new(HashMap::new()),
            next_screen_request: AtomicU64::new(1),
            run_history: Mutex::new(Vec::new()),
            run_captures: Mutex::new(HashMap::new()),
            run_waiters: Mutex::new(HashMap::new()),
            pr_cache: Mutex::new(HashMap::new()),
            restored_panes: Mutex::new(restored_panes),
            session_dirty: AtomicBool::new(false),
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(focused) = event {
                let state = window.state::<AppState>();
                state
                    .window_focused
                    .store(*focused, std::sync::atomic::Ordering::Relaxed);
            }
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            setup_menu(app.handle())?;
            intel::spawn(app.handle().clone());
            config_watch::spawn(app.handle().clone());
            spawn_session_saver(app.handle().clone());
            #[cfg(unix)]
            server::spawn(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::workspace_snapshot,
            commands::get_config,
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
            commands::list_notifications,
            commands::mark_all_notifications_read,
            commands::resolve_screen_read,
            commands::store_scrollback,
            commands::load_scrollback,
            commands::write_pty,
            commands::resize_pty,
            commands::ack_pty,
        ])
        .build(tauri::generate_context!())
        .expect("error while running cmux")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                save_session(&app.state::<AppState>());
                #[cfg(unix)]
                {
                    if let Some(disc) = cmux_core::ipc::read_discovery() {
                        if disc.pid == std::process::id() {
                            let _ = std::fs::remove_file(&disc.socket);
                            cmux_core::ipc::remove_discovery();
                        }
                    }
                }
            }
        });
}

/// Debounced session writer: persists at most once a second, only when
/// the workspace actually changed. Also asks the webview to serialize
/// scrollback every 30s — driven from Rust because WKWebView suspends JS
/// timers in occluded windows.
fn spawn_session_saver(handle: tauri::AppHandle) {
    use tauri::Emitter;
    std::thread::spawn(move || {
        let mut tick: u64 = 0;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            tick += 1;
            let state = handle.state::<AppState>();
            if state.session_dirty.swap(false, Ordering::Relaxed) {
                save_session(&state);
            }
            if tick.is_multiple_of(30) {
                let _ = handle.emit("scrollback-save-request", ());
            }
        }
    });
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

