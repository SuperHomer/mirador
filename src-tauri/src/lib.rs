mod agent_editor;
// build.rs's list; compiled here only for the test that checks it.
#[cfg(test)]
mod app_commands;
mod browser;
mod checkpoints;
mod commands;
mod config_watch;
mod markdown_watch;
mod handoff;
mod intel;
mod notify;
mod runs;
mod send_files;
mod server;
mod settings;
mod tasks;
mod teams;
mod telegram;
mod update;
mod whatsnew;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use cmux_core::pty::PtyManager;
use cmux_core::state::{PaneMeta, Workspace};
use cmux_protocol::{NotificationDto, ResolvedConfig, RunRecord};
use tauri::{Emitter, Manager};

/// A pane's screen as the webview reads it (`server::read_screen_at`).
pub struct ScreenRead {
    pub text: String,
    /// The cursor as (row, column), when its row is among the text's lines.
    pub cursor: Option<(usize, usize)>,
    /// The cursor's row from the cursor on, its dim cells blanked: what a
    /// harness drew there at normal intensity (`cmux_core::handoff::has_draft`).
    pub after_cursor: String,
}

pub struct AppState {
    pub pty: PtyManager,
    pub workspace: Mutex<Workspace>,
    pub meta: Mutex<HashMap<String, PaneMeta>>,
    pub config: Mutex<ResolvedConfig>,
    pub notifications: Mutex<Vec<NotificationDto>>,
    pub window_focused: AtomicBool,
    /// Pending read-screen round-trips to the webview.
    pub screen_requests: Mutex<HashMap<u64, std::sync::mpsc::Sender<ScreenRead>>>,
    pub next_screen_request: AtomicU64,
    /// The person's keystrokes held from panes a handoff is typing into
    /// (`handoff::Typing`). `write_pty` writes under this lock, so a held
    /// key handed back cannot be overtaken by one typed after it.
    pub key_hold: Mutex<cmux_core::handoff::KeyHold>,
    /// When the person last typed into each pane: keys, a paste, a drop,
    /// `mira send-input` — not what the terminal reports of its own accord
    /// (`cmux_core::handoff::is_typing`).
    pub typed_at: Mutex<HashMap<String, std::time::Instant>>,
    /// Messages `mira agent send` queued, per pane, oldest first, while
    /// the person was in the way there (`handoff::send`).
    pub queued: Mutex<HashMap<String, std::collections::VecDeque<handoff::Message>>>,
    pub next_queued: AtomicU64,
    /// When each queued message was typed (ms since the epoch), for `send
    /// --wait` to wait for the turn it started. 16 bytes a queued message,
    /// for the app's life, and not pruned: a `--wait` may ask after its
    /// message any time, and nothing says when the last one has.
    pub delivered: Mutex<HashMap<u64, u64>>,
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
    /// The files markdown panes follow, and the watcher on their directories.
    pub markdown_watch: markdown_watch::MarkdownWatch,
    /// Claude Code transcripts read so far, for agent activity.
    pub transcripts: Mutex<cmux_core::transcript::Cache>,
    /// Claude Code transcripts and Codex rollouts read so far, for agent
    /// trails.
    pub trails: Mutex<cmux_core::transcript::Cache<cmux_core::trail::Trail>>,
    pub codex_trails: Mutex<cmux_core::transcript::Cache<cmux_core::codex_trail::CodexTrail>>,
    /// Codex rollouts read so far, for agent activity.
    pub codex_stats: Mutex<cmux_core::transcript::Cache<cmux_core::codex_trail::CodexStats>>,
    /// What every recent transcript and rollout counted, for the accounts'
    /// usage: read on from where it stopped each time it is asked.
    pub usage: Mutex<cmux_core::usage::UsageCache>,
    /// The file each pane's agent last wrote its trail to, by pane: kept
    /// after the agent exits, until another starts there or the pane
    /// closes.
    pub trail_files: Mutex<std::collections::HashMap<String, commands::TrailFile>>,
    /// Each live Codex pane's rollout, followed for the ends of its turns.
    pub codex_tails: Mutex<std::collections::HashMap<String, cmux_core::codex_trail::RolloutTail>>,
    /// The task log's path, locked while it is appended to or read: the
    /// app is its one writer.
    pub task_log: Mutex<std::path::PathBuf>,
    /// The palette's actions, as the page last sent them: the menu bar is
    /// built from them, and again from them when a key is rebound.
    pub menu_actions: Mutex<Vec<cmux_protocol::MenuAction>>,
    /// Telegram bots bridged to agents, by pane: in memory only, never
    /// saved, since each holds its bot's token.
    pub telegram: telegram::Bridges,
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
    // session; `codex resume <id>` for a Codex) typed into it — no
    // keypress, and exiting Claude leaves you at a prompt rather than
    // offering to run it again. The one exception
    // to restored panes waiting for a keypress: resuming a conversation
    // runs nothing until you type into it. Not on Windows, which cannot
    // see Claude exit and so would resume closed conversations forever —
    // see `cmux_core::agents::AUTO_RESUME`.
    // A role's pane resumes on the role's model and effort — looked up
    // where the pane works, never taken from the session file, which only
    // names the role. Its prompt file is passed again too: a conversation
    // keeps its prompt only until it is compacted.
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
        // A pane whose Codex the poller saw on a conversation resumes it as
        // a Codex, with its role; one started on a Pi's or an OpenCode's
        // conversation as that; any other is resumed as Claude Code.
        let resumes = m.agent_session.as_deref().map_or(cmux_protocol::Harness::Claude, cmux_core::agents::session_harness);
        let codex = resumes == cmux_protocol::Harness::Codex;
        // Its own launch line for a harness that keeps nothing of it in a
        // conversation: Codex, Pi, OpenCode.
        let relaunched = resumes != cmux_protocol::Harness::Claude;
        if let Some(r) = role.as_ref().filter(|r| !relaunched && r.harness != resumes) {
            other_harness.insert(id.clone(), r.harness);
        }
        let role = role.filter(|r| r.harness == resumes);
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
        // A prompt file gone since is left out rather than the resume: the
        // conversation still has its prompt until it is compacted, and a
        // prompt, unlike limits, holds nothing back.
        let prompt = m.agent_prompt.as_deref().filter(|f| std::path::Path::new(f).is_file());
        let quoted_prompt = prompt.map(cmux_core::pty::shell::quote_arg);
        // A Codex's, a Pi's or an OpenCode's flags are the line that would
        // start it fresh, built as a launch builds it, with no task.
        let launch = relaunched.then(|| {
            let socket = codex.then(|| crate::commands::codex_socket(role.as_ref(), m.cwd.as_deref())).flatten();
            let notify = codex.then(|| crate::commands::codex_notify(role.as_ref())).flatten();
            cmux_core::agents::launch_command(
                role.as_ref(),
                resumes,
                None,
                prompt.map(std::path::Path::new),
                None,
                None,
                socket.as_ref().map(|(base, socket)| (base.as_str(), socket.as_path())),
                notify.as_deref(),
                None,
                cmux_core::pty::shell::quote_arg,
            )
        });
        if relaunched {
            // That harness's agent from the start, as a launch marks one:
            // on Windows no process table ever says so.
            m.agent_harness = Some(resumes);
        }
        let restored = cmux_core::agents::restore_pane(
            m.command.as_deref(),
            m.remote_host.is_some(),
            m.agent_session.as_deref().filter(|_| !settings_gone),
            cmux_core::agents::ResumeFlags {
                model: quoted_model.as_deref(),
                effort: quoted_effort.as_deref(),
                settings: quoted_settings.as_deref(),
                prompt: quoted_prompt.as_deref(),
                launch: launch.as_deref(),
            },
            cmux_core::agents::AUTO_RESUME,
        );
        m.command = restored.command;
        m.startup_input = restored.startup_input;
    }
    // Launch files nobody has started an agent with in a month — except
    // the settings and prompts a restored pane is about to pass again.
    let keep: Vec<std::path::PathBuf> = meta
        .values()
        .flat_map(|m| [&m.agent_settings, &m.agent_prompt])
        .filter_map(|f| f.as_ref().map(std::path::PathBuf::from))
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
    // A role that starts something other than Claude Code, with no Codex
    // thread recorded, has no session to resume. Held, its agent is still
    // running, and is that harness from the start; otherwise the role no
    // longer marks the pane, which would pass for a Claude agent until the
    // poller said otherwise (and on Windows, which lists no processes, it
    // never does).
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
            key_hold: Mutex::new(Default::default()),
            typed_at: Mutex::new(HashMap::new()),
            queued: Mutex::new(HashMap::new()),
            next_queued: AtomicU64::new(1),
            delivered: Mutex::new(HashMap::new()),
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
            usage: Mutex::new(Default::default()),
            trail_files: Mutex::new(Default::default()),
            codex_tails: Mutex::new(Default::default()),
            task_log: Mutex::new(task_log),
            update: update::UpdateState::default(),
            emit_coalescer: commands::EmitCoalescer::default(),
            markdown_watch: Default::default(),
            menu_actions: Mutex::new(Vec::new()),
            telegram: Mutex::new(HashMap::new()),
        })
        // The menu bar's actions run in the page, as the palette's do.
        .on_menu_event(|app, event| {
            if let Some(action) = event.id().0.strip_prefix(MENU_ACTION) {
                let _ = app.emit_to("main", "menu-action", action);
            }
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
            set_menu(app.handle())?;
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
            // Pastes are for the moment they are pasted in; a day later
            // they are clutter, and maybe private clutter.
            std::thread::spawn(|| {
                let day = std::time::Duration::from_secs(24 * 60 * 60);
                cmux_core::paste::prune(&cmux_core::paste::local_dir(), day, std::time::SystemTime::now());
            });
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
            commands::set_tab_color,
            commands::open_in_editor,
            commands::set_menu_actions,
            commands::move_tab,
            commands::split_pane,
            commands::close_pane,
            commands::focus_pane,
            commands::focus_direction,
            commands::paste_image,
            commands::send_files,
            commands::set_split_ratios,
            commands::swap_pane,
            commands::move_pane,
            commands::rotate_panes,
            commands::equalize_panes,
            commands::apply_layout,
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
            commands::open_markdown,
            commands::open_file_tree,
            commands::load_file_tree,
            commands::open_tree_file_in_editor,
            commands::open_tree_file_diff,
            commands::read_markdown,
            commands::list_markdown,
            markdown_watch::watch_markdown,
            markdown_watch::unwatch_markdown,
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
            settings::open_settings,
            settings::settings_view,
            settings::set_config_value,
            settings::zoom_terminals,
            settings::open_computers,
            settings::list_computers,
            settings::add_computer,
            settings::update_computer,
            settings::remove_computer,
            settings::test_computer,
            teams::start_team,
            teams::list_teams,
            teams::team_cost,
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
            commands::account_usage,
            commands::open_usage,
            commands::mark_pane_read,
            commands::zoom_pane,
            commands::load_graph,
            commands::graph_show_commit,
            whatsnew::whats_new,
            whatsnew::release_image,
            whatsnew::open_whats_new,
            commands::open_ssh,
            commands::start_where,
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

/// The prefix of a menu item's id that names a palette action.
const MENU_ACTION: &str = "action:";

/// The menu bar: the palette's actions, laid out by `cmux_core::menu`
/// with the keybindings' shortcuts, around the macOS app and Edit menus.
/// Until the page has sent its actions there are none, so the menu is
/// never missing at launch.
///
/// There is no File > Close Window: Cmd+W must reach the keymap to close a
/// *pane*, not the window. Edit keeps the roles, so Cmd+C/V reach the
/// webview's clipboard machinery.
///
/// An item's shortcut does not run its action twice. WKWebView hands a
/// keypress to the page before the menu, and the menu sees it only if the
/// page did not take it; `useKeymap` takes every key it binds. So the
/// shortcut runs once, from the keymap, and the menu's runs only what the
/// page let through.
#[cfg(target_os = "macos")]
pub fn set_menu(handle: &tauri::AppHandle) -> tauri::Result<()> {
    use cmux_core::menu::{self, MenuItem as Item};
    use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};

    let state = handle.state::<AppState>();
    let actions = state.menu_actions.lock().unwrap().clone();
    let keybindings = state.config.lock().unwrap().keybindings.clone();
    let mut bar = menu::menus(&actions, &keybindings);

    let append = |submenu: &Submenu<tauri::Wry>, items: &[Item]| -> tauri::Result<()> {
        for item in items {
            match item {
                Item::Action { id, label, accelerator } => submenu.append(&MenuItem::with_id(
                    handle,
                    format!("{MENU_ACTION}{id}"),
                    label,
                    true,
                    accelerator.as_deref(),
                )?)?,
                Item::Separator => submenu.append(&PredefinedMenuItem::separator(handle)?)?,
            }
        }
        Ok(())
    };

    let app_menu = Submenu::new(handle, "Mirador", true)?;
    app_menu.append(&PredefinedMenuItem::about(handle, None, Some(AboutMetadata::default()))?)?;
    app_menu.append(&PredefinedMenuItem::separator(handle)?)?;
    if let Some(i) = bar.iter().position(|m| m.title == "Mirador") {
        append(&app_menu, &bar.remove(i).items)?;
        app_menu.append(&PredefinedMenuItem::separator(handle)?)?;
    }
    app_menu.append(&PredefinedMenuItem::hide(handle, None)?)?;
    app_menu.append(&PredefinedMenuItem::hide_others(handle, None)?)?;
    app_menu.append(&PredefinedMenuItem::separator(handle)?)?;
    app_menu.append(&PredefinedMenuItem::quit(handle, None)?)?;

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
    for m in &bar {
        let submenu = Submenu::new(handle, &m.title, true)?;
        append(&submenu, &m.items)?;
        menu.append(&submenu)?;
    }
    menu.append(&Submenu::with_items(
        handle,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(handle, None)?,
            &PredefinedMenuItem::maximize(handle, None)?,
        ],
    )?)?;
    handle.set_menu(menu)?;
    Ok(())
}

/// Windows and Linux get no menu bar: their keys are the keymap's alone.
#[cfg(not(target_os = "macos"))]
pub fn set_menu(_handle: &tauri::AppHandle) -> tauri::Result<()> {
    Ok(())
}

