mod agent_editor;
// build.rs's list; compiled here only for the test that checks it.
#[cfg(test)]
mod app_commands;
mod browser;
mod commands;
mod config_watch;
mod handoff;
mod intel;
mod notify;
mod runs;
mod server;
mod tasks;
mod teams;
mod update;
mod whatsnew;

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
    /// Panes whose session holder outlived the last quit, still to be
    /// reattached (each leaves the set on its first attach).
    pub pending_reattach: Mutex<HashSet<String>>,
    /// The same panes, for the whole run: their saved scrollback is older
    /// than what the holder replays, so it is not shown.
    pub held_at_launch: HashSet<String>,
    /// Pending browser automation round-trips.
    pub browser_bridge: browser::BrowserBridge,
    /// Set on every workspace change; the saver thread persists and clears.
    pub session_dirty: AtomicBool,
    /// Update offered by the last check, if any.
    pub update: update::UpdateState,
    /// Throttles snapshots triggered by terminal output (titles, cwd).
    pub emit_coalescer: commands::EmitCoalescer,
    /// Claude Code transcripts read so far, for agent activity.
    pub transcripts: Mutex<cmux_core::transcript::Cache>,
    /// Claude Code transcripts and Codex rollouts read so far, for agent
    /// trails.
    pub trails: Mutex<cmux_core::transcript::Cache<cmux_core::trail::Trail>>,
    pub codex_trails: Mutex<cmux_core::transcript::Cache<cmux_core::codex_trail::CodexTrail>>,
    /// Codex rollouts read so far, for agent activity.
    pub codex_stats: Mutex<cmux_core::transcript::Cache<cmux_core::codex_trail::CodexStats>>,
    /// The file each pane's agent last wrote its trail to, by pane: kept
    /// after the agent exits, until another starts there or the pane
    /// closes.
    pub trail_files: Mutex<std::collections::HashMap<String, commands::TrailFile>>,
    /// Each live Codex pane's rollout, followed for the ends of its turns.
    pub codex_tails: Mutex<std::collections::HashMap<String, cmux_core::codex_trail::RolloutTail>>,
    /// The task log's path, locked while it is appended to or read: the
    /// app is its one writer.
    pub task_log: Mutex<std::path::PathBuf>,
}

pub fn save_session(state: &AppState) {
    let file = {
        let ws = state.workspace.lock().unwrap();
        let meta = state.meta.lock().unwrap();
        cmux_core::session::capture(&ws, &meta)
    };
    if let Err(e) = cmux_core::session::save(&file) {
        eprintln!("mirador: session save failed: {e}");
    }
}

/// The panes a live session holder still serves. Holders the session does
/// not name — left by a crash, or by an app that died before saving a pane
/// it had just opened — are given tabs of their own at the end rather than
/// killed: those are the times their work matters most.
fn live_holders(workspace: &mut Workspace) -> HashSet<String> {
    let live = cmux_core::hold::live_panes();
    for orphan in cmux_core::hold::orphans(&live, &workspace.all_pane_ids()) {
        workspace.adopt_pane(&orphan);
    }
    live.into_iter().collect()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    cmux_core::config::write_default_if_missing();
    let (cfg, cfg_err) = cmux_core::config::load();
    if let Some(err) = &cfg_err {
        eprintln!("mirador: config error, using defaults: {err}");
    }

    // Restore the previous session; command panes come back idle.
    let restored = cmux_core::session::load().and_then(cmux_core::session::restore);
    let (workspace, mut meta) = match restored {
        Some(restored) => restored,
        // First launch, or a session file that no longer loads: the single
        // pane is as new as one in a new tab, so `defaultCwd` applies.
        None => {
            let workspace = Workspace::default();
            let mut meta: HashMap<String, PaneMeta> = HashMap::new();
            if let Some(cwd) = cmux_core::config::default_cwd() {
                meta.entry(workspace.focused_pane()).or_default().cwd = Some(cwd);
            }
            (workspace, meta)
        }
    };
    // Panes that ran Claude Code come back as the shell they were, with
    // `claude --resume <id>` (or `claude attach <job>` for a daemon
    // session) typed into it — no keypress, and exiting Claude leaves you
    // at a prompt rather than offering to run it again. The one exception
    // to restored panes waiting for a keypress: resuming a conversation
    // runs nothing until you type into it. Not on Windows, which cannot
    // see Claude exit and so would resume closed conversations forever —
    // see `cmux_core::agents::AUTO_RESUME`.
    // A role's pane resumes on the role's model and effort — looked up
    // where the pane works, never taken from the session file, which only
    // names the role. Its prompt is not given again: a resumed
    // conversation keeps the one it started with.
    let mut catalogs: HashMap<Option<String>, cmux_protocol::RoleCatalog> = HashMap::new();
    let mut other_harness: HashMap<String, cmux_protocol::Harness> = HashMap::new();
    for (id, m) in meta.iter_mut() {
        let role = m.agent_role.as_deref().and_then(|name| {
            catalogs
                .entry(m.cwd.clone())
                .or_insert_with(|| cmux_core::roles::catalog(m.cwd.as_deref().map(std::path::Path::new)))
                .roles
                .iter()
                .find(|r| r.name == name)
                .cloned()
        });
        if let Some(r) = role.as_ref().filter(|r| r.harness != cmux_protocol::Harness::Claude) {
            other_harness.insert(id.clone(), r.harness);
        }
        let role = role.filter(|r| r.harness == cmux_protocol::Harness::Claude);
        let role_model = role.as_ref().and_then(|r| r.model.clone());
        m.agent_model = role_model.clone();
        m.agent_effort = role.as_ref().and_then(|r| r.effort.clone());
        let quoted_model = role_model.as_deref().map(cmux_core::pty::shell::quote_arg);
        let quoted_effort = m.agent_effort.as_deref().map(cmux_core::pty::shell::quote_arg);
        // Its limits too, from the file it started with. A file gone since
        // drops the pane's resume rather than its limits: an agent meant
        // to be held to some tools must not come back with all of them.
        let settings = m.agent_settings.clone();
        let settings_gone = settings.as_deref().is_some_and(|f| !std::path::Path::new(f).is_file());
        let quoted_settings = settings.as_deref().map(cmux_core::pty::shell::quote_arg);
        let restored = cmux_core::agents::restore_pane(
            m.command.as_deref(),
            m.remote_host.is_some(),
            m.agent_session.as_deref().filter(|_| !settings_gone),
            quoted_model.as_deref(),
            quoted_effort.as_deref(),
            quoted_settings.as_deref(),
            cmux_core::agents::AUTO_RESUME,
        );
        m.command = restored.command;
        m.startup_input = restored.startup_input;
    }
    // Launch files nobody has started an agent with in a month — except
    // the settings a restored pane is about to pass again.
    let keep: Vec<std::path::PathBuf> = meta
        .values()
        .filter_map(|m| m.agent_settings.as_ref().map(std::path::PathBuf::from))
        .collect();
    cmux_core::roles::prune_prompts(&keep);
    // Panes whose holder survived the quit are still running: nothing about
    // them is restored, so no resume is typed and no keypress is waited for.
    // Checked whatever `persistSessions` says now — turning it off must not
    // strand processes that are already running.
    let mut workspace = workspace;
    let held_at_launch = live_holders(&mut workspace);
    for id in &held_at_launch {
        if let Some(m) = meta.get_mut(id) {
            m.startup_input = None;
        }
    }
    // A role that starts something other than Claude Code — Codex — has no
    // session to resume. Held, its agent is still running, and is that
    // harness from the start; otherwise the role no longer marks the pane,
    // which would pass for a Claude agent until the poller said otherwise
    // (and on Windows, which lists no processes, it never does).
    for (id, harness) in other_harness {
        if let Some(m) = meta.get_mut(&id) {
            if held_at_launch.contains(&id) {
                m.agent_harness = Some(harness);
            } else {
                m.agent_role = None;
            }
        }
    }
    // An agent pane coming back without its process has this long for a
    // resume to bring Claude back, or for a role to be dropped when there
    // is nothing to resume. A held one is still running: it gets none.
    let now = std::time::Instant::now();
    for (id, m) in meta.iter_mut() {
        if m.is_agent() && !held_at_launch.contains(id) {
            m.agent_started_at = Some(now);
        }
    }
    // Command panes AND remote (SSH) panes come back idle: relaunching the
    // app must never re-run a command or silently re-open an SSH session.
    let restored_panes: HashSet<String> = meta
        .iter()
        .filter(|(id, m)| {
            (m.command.is_some() || m.remote_host.is_some()) && !held_at_launch.contains(*id)
        })
        .map(|(id, _)| id.clone())
        .collect();
    cmux_core::session::gc_scrollback(&workspace.all_pane_ids());
    let task_log = cmux_core::tasks::log_path();
    if let Err(e) = cmux_core::tasks::compact(&task_log, runs::now_ms()) {
        eprintln!("mirador: could not compact the task log: {e}");
    }

    let builder = tauri::Builder::default();
    // A second launch on Windows or Linux would run without the automation
    // endpoint, which the first one holds, so `mira` in its panes would
    // drive the wrong app. Release only: a dev build may run beside the
    // installed one. It must be the first plugin.
    // Not macOS: LaunchServices already keeps one instance there, and the
    // plugin's fixed /tmp socket would catch sandboxed release builds.
    #[cfg(all(not(debug_assertions), not(target_os = "macos")))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        if let Some(window) = app.get_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    }));
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
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
            pending_reattach: Mutex::new(held_at_launch.clone()),
            held_at_launch,
            browser_bridge: browser::BrowserBridge::default(),
            session_dirty: AtomicBool::new(false),
            transcripts: Mutex::new(Default::default()),
            trails: Mutex::new(Default::default()),
            codex_trails: Mutex::new(Default::default()),
            codex_stats: Mutex::new(Default::default()),
            trail_files: Mutex::new(Default::default()),
            codex_tails: Mutex::new(Default::default()),
            task_log: Mutex::new(task_log),
            update: update::UpdateState::default(),
            emit_coalescer: commands::EmitCoalescer::default(),
        })
        .on_window_event(|window, event| {
            match event {
                tauri::WindowEvent::Focused(focused) => {
                    let state = window.state::<AppState>();
                    state
                        .window_focused
                        .store(*focused, std::sync::atomic::Ordering::Relaxed);
                }
                // The host view's origin within the window is memoized, and
                // these are the only events that can move it: a resize, a
                // move between displays, or a scale change.
                tauri::WindowEvent::Resized(_)
                | tauri::WindowEvent::Moved(_)
                | tauri::WindowEvent::ScaleFactorChanged { .. } => {
                    browser::forget_host_origin(window.app_handle());
                }
                _ => {}
            }
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            setup_menu(app.handle())?;
            notify::ensure_permission(app.handle());
            // Before session restore can want it for a rerun.
            cmux_core::pty::shell::warm_interactive_path();
            intel::spawn(app.handle().clone());
            config_watch::spawn(app.handle().clone());
            spawn_session_saver(app.handle().clone());
            commands::spawn_emit_coalescer(app.handle().clone());
            server::spawn(app.handle().clone());
            update::spawn_check_loop(app.handle().clone());
            whatsnew::announce_on_launch(app.handle().clone());
            // Off the startup path: it reads (and may write) ~/.claude.
            std::thread::spawn(commands::repair_claude_hooks);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::workspace_snapshot,
            commands::get_config,
            commands::new_tab,
            commands::close_tab,
            commands::quit_ending_sessions,
            commands::claude_integration_offer,
            commands::setup_claude_integration,
            commands::dismiss_claude_integration,
            commands::set_active_tab,
            commands::rename_tab,
            commands::move_tab,
            commands::split_pane,
            commands::close_pane,
            commands::focus_pane,
            commands::focus_direction,
            commands::set_split_ratios,
            commands::set_view,
            commands::set_camera,
            commands::set_card_rect,
            commands::set_frame_position,
            commands::board_fit,
            commands::board_arrange,
            commands::board_card_camera,
            commands::attach_pane,
            commands::list_notifications,
            commands::mark_all_notifications_read,
            commands::resolve_screen_read,
            commands::store_scrollback,
            commands::load_scrollback,
            commands::open_browser,
            commands::open_diff,
            commands::load_diff,
            commands::set_diff_spec,
            commands::list_worktrees,
            commands::set_diff_worktree,
            commands::open_graph,
            commands::open_agent,
            commands::agent_roles,
            agent_editor::open_agent_editor,
            agent_editor::agent_editor_catalog,
            agent_editor::agent_editor_used_by,
            agent_editor::agent_editor_told,
            agent_editor::agent_file_read,
            agent_editor::agent_file_check,
            agent_editor::agent_file_template,
            agent_editor::agent_file_save,
            agent_editor::agent_file_save_fields,
            agent_editor::agent_file_delete,
            teams::start_team,
            teams::list_teams,
            teams::team_add,
            teams::team_restart,
            tasks::task_cards,
            tasks::task_history,
            tasks::task_add,
            tasks::task_send,
            tasks::send_next_task,
            commands::toggle_agent_wall,
            commands::set_wall_view,
            commands::set_wall_mode,
            commands::set_task_layout,
            commands::set_agents_per_tab,
            commands::get_agent_activity,
            commands::get_agent_trail,
            commands::get_trail_detail,
            commands::mark_pane_read,
            commands::zoom_pane,
            commands::load_graph,
            commands::graph_show_commit,
            whatsnew::whats_new,
            whatsnew::release_image,
            whatsnew::open_whats_new,
            commands::open_ssh,
            commands::ssh_hosts,
            commands::set_browser_bounds,
            commands::browser_screenshot,
            commands::set_browser_visible,
            commands::browser_navigate,
            commands::browser_history,
            commands::write_pty,
            commands::resize_pty,
            commands::ack_pty,
            update::app_version,
            update::available_update,
            update::check_update,
            update::install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while running Mirador")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                save_session(&app.state::<AppState>());
                app.state::<AppState>().pty.finish_kills();
                // Only the instance that owns the endpoint clears it.
                if let Some(disc) = cmux_core::ipc::read_discovery() {
                    if disc.pid == std::process::id() {
                        cmux_core::transport::cleanup(&disc.socket);
                        cmux_core::ipc::remove_discovery();
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
        "Mirador",
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

