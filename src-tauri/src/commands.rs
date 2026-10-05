use std::sync::{Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State};

use cmux_core::osc::{OscEvent, OscScanner};
use cmux_protocol::{Direction, NotificationDto, SplitDir, WorkspaceSnapshot};

use crate::{notify, runs, AppState};

#[derive(Clone, Serialize)]
struct PaneExitPayload {
    pane_id: String,
    exit_code: Option<i32>,
    /// True for command panes: a keypress reruns instead of opening a shell.
    is_command: bool,
    /// True for SSH panes: a keypress reconnects.
    is_remote: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewTabResult {
    pub tab_id: String,
    pub pane_id: String,
}

fn build_snapshot(state: &AppState) -> WorkspaceSnapshot {
    let mut snapshot = {
        let ws = state.workspace.lock().unwrap();
        let meta = state.meta.lock().unwrap();
        ws.snapshot(&meta)
    };
    notify::decorate_snapshot(state, &mut snapshot);
    snapshot.agent_panes = {
        let meta = state.meta.lock().unwrap();
        meta.iter()
            .filter_map(|(pane, m)| {
                m.command.as_ref().map(|c| cmux_protocol::AgentPane {
                    pane_id: pane.clone(),
                    command: c.clone(),
                })
            })
            .collect()
    };
    // PR status per tab, from the focused pane's (repo, branch).
    {
        let meta = state.meta.lock().unwrap();
        let pr_cache = state.pr_cache.lock().unwrap();
        for tab in &mut snapshot.tabs {
            let key = meta.get(&tab.focused_pane).and_then(|m| {
                Some((m.repo_root.clone()?, m.branch.clone()?))
            });
            if let Some(key) = key {
                tab.pr = pr_cache.get(&key).cloned().flatten();
            }
        }
    }
    snapshot.browser_panes = {
        let meta = state.meta.lock().unwrap();
        meta.iter()
            .filter_map(|(pane, m)| {
                m.browser_url.as_ref().map(|u| cmux_protocol::BrowserPane {
                    pane_id: pane.clone(),
                    url: u.clone(),
                })
            })
            .collect()
    };
    snapshot.diff_panes = {
        let meta = state.meta.lock().unwrap();
        meta.iter()
            .filter_map(|(pane, m)| {
                Some(cmux_protocol::DiffPane {
                    pane_id: pane.clone(),
                    repo: m.diff_repo.clone()?,
                    spec: m.diff_spec.clone()?,
                })
            })
            .collect()
    };
    snapshot.whats_new_panes = {
        let meta = state.meta.lock().unwrap();
        meta.iter()
            .filter_map(|(pane, m)| {
                m.whats_new.as_ref().map(|v| cmux_protocol::WhatsNewPane {
                    pane_id: pane.clone(),
                    version: v.clone(),
                })
            })
            .collect()
    };
    snapshot.graph_panes = {
        let meta = state.meta.lock().unwrap();
        meta.iter()
            .filter_map(|(pane, m)| {
                m.graph_repo.as_ref().map(|r| cmux_protocol::GraphPane {
                    pane_id: pane.clone(),
                    repo: r.clone(),
                })
            })
            .collect()
    };
    snapshot.remote_panes = {
        let meta = state.meta.lock().unwrap();
        meta.iter()
            .filter_map(|(pane, m)| {
                m.remote_host.as_ref().map(|h| cmux_protocol::RemotePane {
                    pane_id: pane.clone(),
                    host: cmux_core::ssh::display_host(h),
                })
            })
            .collect()
    };
    snapshot
}

pub fn emit_workspace(app: &AppHandle) {
    let state = app.state::<AppState>();
    state
        .session_dirty
        .store(true, std::sync::atomic::Ordering::Relaxed);
    broadcast_workspace(app);
}

/// Sends a fresh snapshot without scheduling a session save: for changes
/// the session file doesn't hold, like derived pane titles.
fn broadcast_workspace(app: &AppHandle) {
    let state = app.state::<AppState>();
    let snapshot = build_snapshot(&state);
    let _ = app.emit("workspace-changed", snapshot);
}

/// Minimum gap between snapshots requested through `request_workspace_emit`.
const COALESCED_EMIT_INTERVAL: Duration = Duration::from_millis(100);

/// Pending flag for snapshot changes driven by terminal output. Agent CLIs
/// animate their window title several times a second; a full snapshot per
/// frame, per pane, re-rendered every pane of every tab and kept the webview
/// busy enough to stop responding once a few agents ran side by side.
#[derive(Default)]
pub struct EmitCoalescer {
    pending: Mutex<bool>,
    cond: Condvar,
}

/// Coalesced snapshot broadcast: the first request after a quiet period
/// goes out at once, later ones fold into at most one emit per interval.
/// For high-frequency sources only — user actions keep the immediate emit.
/// It does not schedule a session save (an animated title would rewrite
/// the session file every second); callers changing persisted state set
/// `session_dirty` themselves.
pub fn request_workspace_emit(app: &AppHandle) {
    let state = app.state::<AppState>();
    *state.emit_coalescer.pending.lock().unwrap() = true;
    state.emit_coalescer.cond.notify_one();
}

pub fn spawn_emit_coalescer(handle: AppHandle) {
    std::thread::spawn(move || loop {
        {
            let state = handle.state::<AppState>();
            let mut pending = state.emit_coalescer.pending.lock().unwrap();
            while !*pending {
                pending = state.emit_coalescer.cond.wait(pending).unwrap();
            }
            *pending = false;
        }
        broadcast_workspace(&handle);
        std::thread::sleep(COALESCED_EMIT_INTERVAL);
    });
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

/// `command`, when set, makes the new pane a command pane (its PTY runs
/// the command directly; exit is detected, output captured).
#[tauri::command]
pub fn new_tab(
    app: AppHandle,
    state: State<'_, AppState>,
    command: Option<String>,
) -> NewTabResult {
    let (tab_id, pane_id) = state.workspace.lock().unwrap().new_tab();
    // A fresh tab has no pane to inherit a directory from the way a split
    // does, so its one pane starts in `defaultCwd` when that is configured.
    {
        let mut meta = state.meta.lock().unwrap();
        let entry = meta.entry(pane_id.clone()).or_default();
        entry.cwd = cmux_core::config::default_cwd();
        entry.command = command;
    }
    emit_workspace(&app);
    NewTabResult { tab_id, pane_id }
}

#[tauri::command]
pub fn close_tab(app: AppHandle, state: State<'_, AppState>, tab_id: String) {
    let before = state.workspace.lock().unwrap().all_pane_ids();
    let killed = state.workspace.lock().unwrap().close_tab(&tab_id);
    seed_recreated_panes(&state, &before);
    kill_panes(&app, &state, &killed);
    emit_workspace(&app);
}

/// Closing the last tab makes the workspace recreate an empty one, whether
/// the tab was closed directly or by closing its last pane. That pane is as
/// new as a new tab's, so it opens in `defaultCwd` — and since the workspace
/// creates it internally, it is spotted by diffing pane ids rather than
/// threaded back out through two return types.
fn seed_recreated_panes(state: &AppState, before: &[String]) {
    let fresh: Vec<String> = {
        let ws = state.workspace.lock().unwrap();
        ws.all_pane_ids()
            .into_iter()
            .filter(|p| !before.contains(p))
            .collect()
    };
    if fresh.is_empty() {
        return;
    }
    let cwd = cmux_core::config::default_cwd();
    let mut meta = state.meta.lock().unwrap();
    for pane in fresh {
        meta.entry(pane).or_default().cwd = cwd.clone();
    }
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
    // Inherit the source pane's cwd so the new pane opens there.
    {
        let mut meta = state.meta.lock().unwrap();
        let inherited_cwd = meta.get(&pane_id).and_then(|m| m.cwd.clone());
        let entry = meta.entry(new_pane.clone()).or_default();
        entry.cwd = inherited_cwd;
        entry.command = command;
    }
    emit_workspace(&app);
    Ok(new_pane)
}

#[tauri::command]
pub fn close_pane(app: AppHandle, state: State<'_, AppState>, pane_id: String) {
    let before = state.workspace.lock().unwrap().all_pane_ids();
    let killed = state.workspace.lock().unwrap().close_pane(&pane_id);
    seed_recreated_panes(&state, &before);
    kill_panes(&app, &state, &killed);
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

/// Serializes attaches. The command is async so a ConPTY spawn (tens to
/// hundreds of ms) stays off the UI thread when a restored session mounts
/// many panes at once; holding this keeps the old one-at-a-time ordering,
/// so two attaches for one pane can never both see it idle and both spawn.
/// Taken on a blocking-pool thread: queued attaches waiting on it inside
/// the async runtime would tie up its workers, and `write_pty`/`ack_pty`
/// run there too.
static ATTACH_LOCK: Mutex<()> = Mutex::new(());

/// Connects a mounted frontend pane to its PTY: spawns on first attach
/// (or after exit), swaps the output channel on remount. Returns
/// "spawned", "reattached", or "restored" (command pane from a previous
/// session: left idle, keypress reruns).
///
/// `rerun` is the frontend saying a keypress asked for it. Mounting, a
/// remount, or any other attach leaves a session-restored pane idle — only
/// the user starts last session's command.
#[tauri::command]
pub async fn attach_pane(
    app: AppHandle,
    pane_id: String,
    cols: u16,
    rows: u16,
    on_data: Channel<InvokeResponseBody>,
    rerun: bool,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _serialized = ATTACH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        attach_pane_blocking(&app, pane_id, cols, rows, on_data, rerun)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn attach_pane_blocking(
    app: &AppHandle,
    pane_id: String,
    cols: u16,
    rows: u16,
    on_data: Channel<InvokeResponseBody>,
    rerun: bool,
) -> Result<String, String> {
    let state = app.state::<AppState>();
    // Remote panes run `ssh -tt <host>`; command panes run their command
    // directly in the PTY (exit detection, output capture); shell panes get
    // an interactive login shell.
    let (cwd, pane_command, remote_host) = {
        let meta = state.meta.lock().unwrap();
        let m = meta.get(&pane_id);
        (
            m.and_then(|m| m.cwd.clone()),
            m.and_then(|m| m.command.clone()),
            m.and_then(|m| m.remote_host.clone()),
        )
    };

    let sink = {
        let sink_app = app.clone();
        let pane = pane_id.clone();
        // An SSH session isn't a "run": no output capture, no history.
        let capture = pane_command.is_some() && remote_host.is_none();
        // A restored agent pane types its resume command once the shell
        // has printed something. Checked from every sink, not just the
        // spawning one, so a remount before the first prompt (StrictMode)
        // cannot lose it; `take` makes it happen once.
        let mut startup_pending = pane_command.is_none() && remote_host.is_none();
        move |bytes: &[u8]| {
            let _ = on_data.send(InvokeResponseBody::Raw(bytes.to_vec()));
            if capture {
                let state = sink_app.state::<AppState>();
                crate::runs::capture_output(&state, &pane, bytes);
            }
            if startup_pending {
                startup_pending = false;
                type_startup_input(&sink_app, &pane);
            }
        }
    };

    if state.pty.is_running(&pane_id) {
        state.pty.set_sink(&pane_id, sink);
        let _ = state.pty.resize(&pane_id, cols, rows);
        return Ok("reattached".into());
    }

    // Session-restored command/remote panes attach idle: never auto-rerun
    // `npm test` (or auto-reconnect an SSH session) just because the app
    // relaunched — a keypress does it.
    let was_restored = state.restored_panes.lock().unwrap().contains(&pane_id)
        && (pane_command.is_some() || remote_host.is_some());
    if was_restored && !rerun {
        return Ok("restored".into());
    }
    state.restored_panes.lock().unwrap().remove(&pane_id);

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
                    // Latch: from here on the poller leaves this pane's
                    // cwd alone, even between prompts.
                    entry.cwd_from_shell = true;
                    if entry.cwd.as_deref() != Some(path.as_str()) {
                        entry.cwd = Some(path);
                        true
                    } else {
                        false
                    }
                };
                if changed {
                    // cwd is restored with the session; titles are not.
                    state
                        .session_dirty
                        .store(true, std::sync::atomic::Ordering::Relaxed);
                    request_workspace_emit(&app);
                }
            }
            OscEvent::Title(title) => {
                let state = app.state::<AppState>();
                let changed = {
                    let mut meta = state.meta.lock().unwrap();
                    let entry = meta.entry(pane_id.clone()).or_default();
                    let title = if title.trim().is_empty()
                        || !cmux_core::state::is_meaningful_title(&title)
                    {
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
                    request_workspace_emit(&app);
                }
            }
        })
    };

    let command_builder = match remote_host.as_deref() {
        // Remote pane: the system ssh handles auth/agent/2FA/ProxyJump.
        Some(host) => Some(cmux_core::ssh::interactive_command(host)),
        None => pane_command
            .as_deref()
            .map(|cmd| cmux_core::pty::shell::run_command(cmd, cwd.as_deref())),
    };

    if remote_host.is_none() {
        if let Some(cmd) = &pane_command {
            crate::runs::record_start(&state, &pane_id, cmd);
        }
    }

    let exit_app = app.clone();
    state.pty.spawn(
        &pane_id,
        cols,
        rows,
        cwd.as_deref(),
        command_builder,
        Box::new(scanner),
        sink,
        move |exited_id, exit_code| {
            let is_command = crate::runs::finish_run(&exit_app, exited_id, exit_code);
            let is_remote = {
                let state = exit_app.state::<AppState>();
                let meta = state.meta.lock().unwrap();
                meta.get(exited_id).is_some_and(|m| m.remote_host.is_some())
            };
            let _ = exit_app.emit(
                "pane-exit",
                PaneExitPayload {
                    pane_id: exited_id.to_string(),
                    exit_code,
                    is_command,
                    is_remote,
                },
            );
        },
    )?;

    Ok("spawned".into())
}

/// Types a restored pane's `startup_input` into its shell. The first output
/// is usually the start of the prompt, not the end of shell startup, so it
/// waits a moment longer; input that still arrives early is not lost —
/// the tty holds it until the line editor reads it.
fn type_startup_input(app: &AppHandle, pane_id: &str) {
    let state = app.state::<AppState>();
    let Some(input) = state
        .meta
        .lock()
        .unwrap()
        .get_mut(pane_id)
        .and_then(|m| m.startup_input.take())
    else {
        return;
    };
    let app = app.clone();
    let pane = pane_id.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        let _ = app
            .state::<AppState>()
            .pty
            .write(&pane, format!("{input}\r").as_bytes());
    });
}

/// Opens a browser pane: split of `pane_id` (or a new tab when `tab` is
/// true). The child webview is created when the frontend reports bounds.
#[tauri::command]
pub fn open_browser(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: Option<String>,
    tab: bool,
    url: String,
) -> Result<String, String> {
    let new_pane = if tab {
        let (_, pane) = state.workspace.lock().unwrap().new_tab();
        pane
    } else {
        let target = pane_id.unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
        state
            .workspace
            .lock()
            .unwrap()
            .split_pane(&target, SplitDir::Row)
            .ok_or_else(|| format!("no pane {target}"))?
    };
    state
        .meta
        .lock()
        .unwrap()
        .entry(new_pane.clone())
        .or_default()
        .browser_url = Some(url);
    emit_workspace(&app);
    Ok(new_pane)
}

/// Opens a diff pane: split of `pane_id` (or a new tab when `tab` is
/// true). The repository comes from the source pane, so `mira diff` needs
/// no path — it reviews whatever repo you (or your agent) are working in.
#[tauri::command]
pub fn open_diff(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: Option<String>,
    tab: bool,
    spec: Option<String>,
    worktree: Option<String>,
) -> Result<String, String> {
    // `mira diff` reports the pane it ran in, so an agent's diff lands on
    // its own repository rather than whichever pane the human is looking
    // at. Fall back to the focused pane when that id is absent (an older
    // CLI, or a lost environment) or stale (the pane has since closed).
    let source = pane_id
        .filter(|id| state.meta.lock().unwrap().contains_key(id))
        .unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    // Resolve the repo before the layout changes: a new tab's pane has no
    // cwd of its own yet, and a split's does not until its shell reports one.
    let repo = source_repo(&state, &source)?;
    // `--worktree` reviews another checkout than the one the calling pane
    // sits in; without it the pane's own checkout is the subject, which is
    // what `mira diff` has always meant.
    let repo = match worktree {
        Some(wanted) => resolve_worktree(std::path::Path::new(&repo), &wanted)?,
        None => repo,
    };

    let spec = spec.unwrap_or_else(|| cmux_core::diff::WORKTREE.to_string());
    cmux_core::diff::verify_spec(std::path::Path::new(&repo), &spec)?;

    let new_pane = if tab {
        let (_, pane) = state.workspace.lock().unwrap().new_tab();
        pane
    } else {
        state
            .workspace
            .lock()
            .unwrap()
            .split_pane(&source, SplitDir::Row)
            .ok_or_else(|| format!("no pane {source}"))?
    };
    {
        let mut meta = state.meta.lock().unwrap();
        let entry = meta.entry(new_pane.clone()).or_default();
        entry.cwd = Some(repo.clone());
        entry.diff_repo = Some(repo);
        entry.diff_spec = Some(spec);
    }
    emit_workspace(&app);
    Ok(new_pane)
}

/// Runs the pane's diff. Async so a `git diff` over a large repository
/// never blocks the UI thread — the pane shows a spinner meanwhile.
#[tauri::command]
pub async fn load_diff(
    state: State<'_, AppState>,
    pane_id: String,
) -> Result<cmux_protocol::DiffResult, String> {
    let (repo, spec) = {
        let meta = state.meta.lock().unwrap();
        let m = meta.get(&pane_id).ok_or_else(|| format!("no pane {pane_id}"))?;
        (
            m.diff_repo.clone().ok_or("not a diff pane")?,
            m.diff_spec.clone().ok_or("not a diff pane")?,
        )
    };
    cmux_core::diff::load(std::path::Path::new(&repo), &spec)
}

/// The repository a *source* pane speaks for: the intel poller's answer
/// when it has one, else the repo containing the pane's cwd. Inside a
/// linked worktree this is that worktree's root, which is what makes
/// `mira diff` review the checkout you ran it in.
fn source_repo(state: &AppState, pane_id: &str) -> Result<String, String> {
    let meta = state.meta.lock().unwrap();
    let m = meta.get(pane_id);
    m.and_then(|m| m.repo_root.clone())
        .or_else(|| {
            let cwd = m.and_then(|m| m.cwd.clone())?;
            Some(cmux_core::git::find_repo_root(&cwd)?.to_string_lossy().to_string())
        })
        .ok_or_else(|| "not inside a git repository".to_string())
}

/// Every checkout of the repository a source pane sits in — the socket's
/// listing, which runs from a terminal pane rather than a diff pane.
pub fn worktrees_for_source(
    state: &AppState,
    pane_id: &str,
) -> Result<Vec<cmux_protocol::Worktree>, String> {
    let repo = source_repo(state, pane_id)?;
    cmux_core::worktree::list(std::path::Path::new(&repo))
}

/// The repository root a diff pane is reading, or an error naming why the
/// pane has none.
fn diff_repo_of(state: &AppState, pane_id: &str) -> Result<String, String> {
    let meta = state.meta.lock().unwrap();
    let m = meta
        .get(pane_id)
        .ok_or_else(|| format!("no pane {pane_id}"))?;
    m.diff_repo.clone().ok_or_else(|| "not a diff pane".into())
}

/// Every checkout of a diff pane's repository, for the pane's worktree
/// picker. Listed on demand rather than polled: `git worktree list` is a
/// process spawn, and worktrees are created by hand, not by the second.
#[tauri::command]
pub fn list_worktrees(
    state: State<'_, AppState>,
    pane_id: String,
) -> Result<Vec<cmux_protocol::Worktree>, String> {
    let repo = diff_repo_of(&state, &pane_id)?;
    cmux_core::worktree::list(std::path::Path::new(&repo))
}

/// Points a diff pane at another checkout of the same repository, by path
/// or by branch name. The spec is left alone, so switching worktrees keeps
/// you on "uncommitted changes" (or whichever view you were reading).
#[tauri::command]
pub fn set_diff_worktree(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    worktree: String,
) -> Result<(), String> {
    let repo = diff_repo_of(&state, &pane_id)?;
    let target = resolve_worktree(std::path::Path::new(&repo), &worktree)?;
    {
        let mut meta = state.meta.lock().unwrap();
        let entry = meta
            .get_mut(&pane_id)
            .ok_or_else(|| format!("no pane {pane_id}"))?;
        entry.diff_repo = Some(target.clone());
        // The pane's cwd follows, so a `mira diff` run from inside the pane
        // and the sidebar's branch both describe the checkout on screen.
        entry.cwd = Some(target);
    }
    state
        .session_dirty
        .store(true, std::sync::atomic::Ordering::Relaxed);
    emit_workspace(&app);
    Ok(())
}

/// Resolves a worktree the caller named against `repo`, rejecting one that
/// cannot be diffed. Shared by the picker and by `mira diff --worktree`, so
/// a branch name means the same thing in the UI and on the command line.
fn resolve_worktree(repo: &std::path::Path, wanted: &str) -> Result<String, String> {
    let worktrees = cmux_core::worktree::list(repo)?;
    let found = cmux_core::worktree::resolve(&worktrees, wanted).ok_or_else(|| {
        let known: Vec<&str> = worktrees
            .iter()
            .map(|w| w.branch.as_deref().unwrap_or(w.path.as_str()))
            .collect();
        format!("no worktree `{wanted}` (have: {})", known.join(", "))
    })?;
    // A bare repository has no files on disk, and a pruned one's directory
    // is gone: either would make `git diff` fail with git's own wording
    // some way into the pane. Say so here instead.
    if found.bare {
        return Err(format!("`{wanted}` is a bare repository: nothing is checked out"));
    }
    if found.prunable {
        return Err(format!(
            "worktree `{wanted}` is stale — its directory is gone (`git worktree prune`)"
        ));
    }
    Ok(found.path.clone())
}

/// Points an existing diff pane at another spec (the pane's own header
/// switcher: uncommitted / staged / HEAD).
#[tauri::command]
pub fn set_diff_spec(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    spec: String,
) -> Result<(), String> {
    {
        let mut meta = state.meta.lock().unwrap();
        let entry = meta.get_mut(&pane_id).ok_or_else(|| format!("no pane {pane_id}"))?;
        if entry.diff_spec.is_none() {
            return Err("not a diff pane".into());
        }
        entry.diff_spec = Some(spec);
    }
    emit_workspace(&app);
    Ok(())
}

/// Opens a commit-graph pane: split of `pane_id` (or a new tab), reading
/// the repository of the pane it was opened from — the same rule as
/// `open_diff`, so both review surfaces follow the pane you work in.
#[tauri::command]
pub fn open_graph(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: Option<String>,
    tab: bool,
) -> Result<String, String> {
    let source = pane_id
        .filter(|id| state.meta.lock().unwrap().contains_key(id))
        .unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    let repo = source_repo(&state, &source)?;

    let new_pane = if tab {
        let (_, pane) = state.workspace.lock().unwrap().new_tab();
        pane
    } else {
        state
            .workspace
            .lock()
            .unwrap()
            .split_pane(&source, SplitDir::Row)
            .ok_or_else(|| format!("no pane {source}"))?
    };
    {
        let mut meta = state.meta.lock().unwrap();
        let entry = meta.entry(new_pane.clone()).or_default();
        entry.cwd = Some(repo.clone());
        entry.graph_repo = Some(repo);
    }
    emit_workspace(&app);
    Ok(new_pane)
}

/// Reads the pane's graph. Async for the same reason as `load_diff`: a
/// `git log` over a large repository must not block the UI thread.
#[tauri::command]
pub async fn load_graph(
    state: State<'_, AppState>,
    pane_id: String,
    limit: Option<usize>,
) -> Result<cmux_protocol::GraphResult, String> {
    let repo = {
        let meta = state.meta.lock().unwrap();
        let m = meta
            .get(&pane_id)
            .ok_or_else(|| format!("no pane {pane_id}"))?;
        m.graph_repo.clone().ok_or("not a graph pane")?
    };
    let limit = limit.unwrap_or(cmux_core::graph::DEFAULT_LIMIT);
    tauri::async_runtime::spawn_blocking(move || {
        cmux_core::graph::load(std::path::Path::new(&repo), limit)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Shows a commit picked in the graph.
///
/// The graph keeps one diff pane and retargets it, because clicking through
/// history is how the graph is read and a pane per commit would bury the
/// graph itself within a dozen clicks. The remembered pane is used only
/// while it still exists and is still a diff pane — closing it means the
/// next click opens a fresh one.
#[tauri::command]
pub fn graph_show_commit(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    sha: String,
) -> Result<String, String> {
    let (repo, existing) = {
        let meta = state.meta.lock().unwrap();
        let m = meta
            .get(&pane_id)
            .ok_or_else(|| format!("no pane {pane_id}"))?;
        (
            m.graph_repo.clone().ok_or("not a graph pane")?,
            m.graph_diff_pane.clone(),
        )
    };
    cmux_core::diff::verify_spec(std::path::Path::new(&repo), &sha)?;

    let reusable = existing.filter(|id| {
        let meta = state.meta.lock().unwrap();
        meta.get(id).is_some_and(|m| m.diff_spec.is_some())
    });
    if let Some(diff_pane) = reusable {
        {
            let mut meta = state.meta.lock().unwrap();
            if let Some(entry) = meta.get_mut(&diff_pane) {
                entry.diff_spec = Some(sha);
            }
        }
        emit_workspace(&app);
        return Ok(diff_pane);
    }

    let diff_pane = open_diff(
        app.clone(),
        state.clone(),
        Some(pane_id.clone()),
        false,
        Some(sha),
        None,
    )?;
    state
        .meta
        .lock()
        .unwrap()
        .entry(pane_id)
        .or_default()
        .graph_diff_pane = Some(diff_pane.clone());
    emit_workspace(&app);
    Ok(diff_pane)
}

/// Opens a remote (SSH) pane: split of `pane_id` (or a new tab). Its PTY
/// runs `ssh -tt <host>`, so auth/agent/2FA behave exactly as in any
/// terminal; disconnect leaves the pane idle and a keypress reconnects.
#[tauri::command]
pub fn open_ssh(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: Option<String>,
    tab: bool,
    host: String,
) -> Result<String, String> {
    if host.trim().is_empty() {
        return Err("ssh host required".into());
    }
    let new_pane = if tab {
        let (_, pane) = state.workspace.lock().unwrap().new_tab();
        pane
    } else {
        let target = pane_id.unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
        state
            .workspace
            .lock()
            .unwrap()
            .split_pane(&target, SplitDir::Row)
            .ok_or_else(|| format!("no pane {target}"))?
    };
    state
        .meta
        .lock()
        .unwrap()
        .entry(new_pane.clone())
        .or_default()
        .remote_host = Some(host.trim().to_string());
    emit_workspace(&app);
    Ok(new_pane)
}

/// Host aliases from ~/.ssh/config, for the palette's SSH entries.
#[tauri::command]
pub fn ssh_hosts() -> Vec<String> {
    cmux_core::ssh::config_hosts()
}

/// Frontend reports a browser pane's rect; creates or repositions the
/// native child webview.
#[tauri::command]
pub fn set_browser_bounds(
    app: AppHandle,
    pane_id: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), String> {
    crate::browser::ensure_webview(&app, &pane_id, x, y, w, h)
}

#[tauri::command]
pub fn set_browser_visible(app: AppHandle, pane_id: String, visible: bool) {
    crate::browser::set_visible(&app, &pane_id, visible);
}

#[tauri::command]
pub fn browser_navigate(app: AppHandle, pane_id: String, url: String) -> Result<(), String> {
    crate::browser::navigate(&app, &pane_id, &url)
}

#[tauri::command]
pub fn browser_history(app: AppHandle, pane_id: String, action: String) -> Result<(), String> {
    crate::browser::history(&app, &pane_id, &action)
}

/// Frontend persists a pane's serialized scrollback (30s tick / blur).
/// Async: up to 10k lines per pane go to disk, which must not happen on the
/// UI thread.
#[tauri::command]
pub async fn store_scrollback(pane_id: String, data: String) {
    if let Err(e) = cmux_core::session::save_scrollback(&pane_id, &data) {
        eprintln!("mirador: scrollback save failed for {pane_id}: {e}");
    }
}

/// Scrollback from the previous session, if any.
#[tauri::command]
pub async fn load_scrollback(pane_id: String) -> Option<String> {
    cmux_core::session::load_scrollback(&pane_id)
}

/// Async so keystrokes never queue behind the UI thread's other work
/// (output delivery, snapshot emits); the frontend keeps one call in flight
/// per pane, which preserves their order.
#[tauri::command]
pub async fn write_pty(app: AppHandle, pane_id: String, data: String) -> Result<(), String> {
    app.state::<AppState>().pty.write(&pane_id, data.as_bytes())
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
pub async fn ack_pty(app: AppHandle, pane_id: String, bytes: u64) {
    app.state::<AppState>().pty.ack(&pane_id, bytes);
}

fn kill_panes(app: &AppHandle, state: &State<'_, AppState>, panes: &[String]) {
    let mut meta = state.meta.lock().unwrap();
    for pane in panes {
        let _ = state.pty.close(pane);
        meta.remove(pane);
    }
    drop(meta);
    notify::forget_panes(state, panes);
    for pane in panes {
        crate::browser::destroy(app, pane);
        runs::forget_pane(state, pane);
        cmux_core::session::delete_scrollback(pane);
    }
}
