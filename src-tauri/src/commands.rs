use std::sync::{Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State};

use cmux_core::osc::{OscEvent, OscScanner};
use cmux_protocol::{BoardPoint, BoardRect, Camera, Direction, NotificationDto, SplitDir, View, WorkspaceSnapshot};

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
                    since: m.whats_new_since.clone(),
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
    snapshot.agent_editor_panes = {
        let meta = state.meta.lock().unwrap();
        meta.iter()
            .filter(|(_, m)| m.agent_editor)
            .map(|(pane, m)| cmux_protocol::AgentEditorPane { pane_id: pane.clone(), cwd: m.cwd.clone() })
            .collect()
    };
    snapshot.agents = agent_infos(state);
    let per_tab = state.config.lock().unwrap().agents_per_tab;
    cmux_core::wall::fill(&mut snapshot, per_tab);
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

/// Every pane running an agent — one seen in its process table, or a
/// Claude Code one that recorded a session or was started and has not
/// exited — in tab and layout order, which is the order the wall draws
/// them in.
pub fn agent_infos(state: &AppState) -> Vec<cmux_protocol::AgentInfo> {
    // Each pane with its tab, and its name in the tab's team if it has one.
    let panes: Vec<(String, String, Option<String>)> = {
        let ws = state.workspace.lock().unwrap();
        ws.tabs
            .iter()
            .flat_map(|t| {
                cmux_core::layout::pane_ids(&t.root).into_iter().map(move |p| {
                    let member = t
                        .team
                        .as_ref()
                        .and_then(|run| run.members.iter().find(|s| s.pane == p))
                        .map(|s| s.name.clone());
                    (t.id.clone(), p, member)
                })
            })
            .collect()
    };
    // Before `meta`: `trail_file` takes these two locks the other way round.
    let rollouts: std::collections::HashSet<String> = state
        .trail_files
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, file)| matches!(file, TrailFile::Codex(_)))
        .map(|(pane, _)| pane.clone())
        .collect();
    let meta = state.meta.lock().unwrap();
    panes
        .into_iter()
        .filter_map(|(tab_id, pane, member)| {
            let m = meta.get(&pane)?;
            let harness = m.harness()?;
            Some(cmux_protocol::AgentInfo {
                tab_id,
                harness,
                member,
                role: m.agent_role.clone(),
                name: m.agent_name.clone(),
                has_activity: m.has_activity(rollouts.contains(&pane)),
                model: m.agent_model.clone(),
                effort: m.agent_effort.clone(),
                status: m.status(),
                message: m.agent_message.clone(),
                asking: m.agent_asking.clone().filter(|_| m.status() == Some(cmux_protocol::AgentStatus::NeedsYou)),
                topic: m.title.as_deref().and_then(|t| {
                    cmux_core::agents::title_topic(t, harness, &[m.agent_role.as_deref(), m.agent_name.as_deref()])
                }),
                since_ms: m.agent_since_ms,
                cwd: m.cwd.clone(),
                branch: m.branch.clone(),
                pane_id: pane,
            })
        })
        .collect()
}

/// A Claude Code agent's progress and its subagents', read from its
/// transcripts. Its hooks say where the session's transcript is; until one
/// has, there is nothing to read. A Codex agent's, from the rollout it
/// holds open: its model and counts, and no tasks or subagents.
pub fn agent_activity(
    state: &AppState,
    pane_id: &str,
) -> Result<cmux_protocol::AgentActivity, String> {
    let harness = {
        let meta = state.meta.lock().unwrap();
        meta.get(pane_id).filter(|m| m.is_agent()).ok_or("not an agent pane")?.harness()
    };
    if harness == Some(cmux_protocol::Harness::Codex) {
        let TrailFile::Codex(known) = trail_file(state, pane_id)? else {
            return Err("no Codex session found".into());
        };
        let session = state
            .codex_stats
            .lock()
            .unwrap()
            .read(&known.path)
            .map(cmux_core::codex_trail::CodexStats::summary)
            .ok_or("its Codex session file cannot be read")?;
        return Ok(cmux_protocol::AgentActivity { pane_id: pane_id.to_string(), session, subagents: Vec::new() });
    }
    let (transcript, live) = {
        let meta = state.meta.lock().unwrap();
        let m = meta.get(pane_id).filter(|m| m.is_agent()).ok_or("not an agent pane")?;
        if m.harness() != Some(cmux_protocol::Harness::Claude) {
            return Err("only Claude Code and Codex agents report their activity".into());
        }
        let transcript = m.agent_transcript.clone().ok_or(
            "no transcript yet: Claude Code reports it with its hooks \
             (the palette's Set Up Claude Code Integration installs them)",
        )?;
        // Where the process table is not read (Windows) a session is
        // never seen running, and its subagents would all look stopped.
        (transcript, m.agent_live || !cmux_core::agents::AUTO_RESUME)
    };
    let (session, subagents) = state.transcripts.lock().unwrap().activity(&transcript, live);
    Ok(cmux_protocol::AgentActivity {
        pane_id: pane_id.to_string(),
        session,
        subagents,
    })
}

/// [`agent_activity`] for the panel, off the main thread: the first read
/// of a long session's transcript takes a moment.
#[tauri::command]
pub async fn get_agent_activity(
    app: AppHandle,
    pane_id: String,
) -> Result<cmux_protocol::AgentActivity, String> {
    tauri::async_runtime::spawn_blocking(move || agent_activity(&app.state::<AppState>(), &pane_id))
        .await
        .map_err(|e| e.to_string())?
}

/// Where an agent's trail is read from.
#[derive(Clone)]
pub enum TrailFile {
    Claude(std::path::PathBuf),
    Codex(cmux_core::codex_trail::KnownRollout),
}

/// How many panes' trail files are kept: a pane's goes when it closes,
/// and this bounds what nothing else would.
const TRAIL_FILES: usize = 256;

/// The file a pane's agent writes what it does to, or why there is none
/// to read: Claude Code's transcript, which its hooks name, or the rollout
/// its Codex holds open. Once the agent is gone, the one it last wrote.
pub fn trail_file(state: &AppState, pane_id: &str) -> Result<TrailFile, String> {
    let agent = {
        let meta = state.meta.lock().unwrap();
        meta.get(pane_id).filter(|m| m.is_agent()).map(|m| (m.harness(), m.agent_transcript.clone(), m.agent_pid))
    };
    let found = match agent {
        None => {
            return state.trail_files.lock().unwrap().get(pane_id).cloned().ok_or_else(|| "not an agent pane".into())
        }
        Some((Some(cmux_protocol::Harness::Claude), transcript, _)) => transcript
            .filter(|p| p.is_file())
            .map(TrailFile::Claude)
            .ok_or_else(|| "no transcript found".to_string()),
        Some((Some(cmux_protocol::Harness::Codex), _, pid)) => codex_rollout(state, pane_id, pid).map(TrailFile::Codex),
        Some(_) => Err("only Claude Code and Codex agents have a trail".into()),
    };
    // The pane's agent now has this file, or none: what an agent before
    // it wrote is not this one's.
    let mut files = state.trail_files.lock().unwrap();
    match &found {
        Ok(file) => {
            if files.len() >= TRAIL_FILES && !files.contains_key(pane_id) {
                let open = state.meta.lock().unwrap();
                files.retain(|pane, _| open.contains_key(pane));
            }
            files.insert(pane_id.to_string(), file.clone());
        }
        Err(_) => {
            files.remove(pane_id);
        }
    }
    found
}

/// The rollout the Codex running in `pane_id` holds open. Finding it runs
/// `lsof`, so what was found is kept while it can still be right: the same
/// process, and no rollout started since (see `needs_lookup`).
fn codex_rollout(
    state: &AppState,
    pane_id: &str,
    pid: Option<u32>,
) -> Result<cmux_core::codex_trail::KnownRollout, String> {
    use cmux_core::codex_trail::{needs_lookup, newest_rollout, open_rollout, sessions_of, KnownRollout};
    if cfg!(not(unix)) {
        return Err("Codex on Windows has no trail yet".into());
    }
    let pid = pid.ok_or("no Codex session found: Codex is not running here")?;
    let known = match state.trail_files.lock().unwrap().get(pane_id) {
        Some(TrailFile::Codex(known)) => Some(known.clone()),
        _ => None,
    };
    let newest = known.as_ref().and_then(|k| sessions_of(&k.path)).and_then(newest_rollout);
    let lookup = needs_lookup(known.as_ref(), pid, newest.as_deref());
    match known {
        Some(known) if !lookup => Ok(known),
        known => match open_rollout(pid) {
            Ok(path) => {
                // The first time, where its sessions are is known only now.
                let newest = newest.or_else(|| sessions_of(&path).and_then(newest_rollout));
                Ok(KnownRollout { pid, path, newest })
            }
            // The same process holding none any more has exited: what it
            // wrote is still its trail.
            Err(_) if known.as_ref().is_some_and(|k| k.pid == pid) => Ok(known.unwrap()),
            Err(0) => Err("no Codex session found".into()),
            Err(_) => Err("more than one Codex session in this pane".into()),
        },
    }
}

/// What a pane's agent did, turn by turn: its last `turns` turns, and any
/// begun since `since_ms` (`trail::window`). No file to read from is no
/// error: the trail is empty, and says why.
pub fn agent_trail(
    state: &AppState,
    pane_id: &str,
    turns: Option<u32>,
    since_ms: Option<u64>,
) -> cmux_protocol::AgentTrail {
    use cmux_core::trail::{version, window, Window};
    // The window of what the file says now, and how far into it that is.
    fn read<T: cmux_core::transcript::Feed>(
        cache: &Mutex<cmux_core::transcript::Cache<T>>,
        path: &std::path::Path,
        turns_of: fn(&T) -> &[cmux_protocol::TrailTurn],
        turns: Option<u32>,
        since_ms: Option<u64>,
    ) -> Option<(Window, String)> {
        let mut cache = cache.lock().unwrap();
        let window = window(turns_of(cache.read(path)?), turns, since_ms);
        let consumed = cache.consumed(path).unwrap_or(0);
        Some((window, version(path, consumed, turns, since_ms)))
    }
    let empty = |reason: String| cmux_protocol::AgentTrail {
        pane_id: pane_id.to_string(),
        turns: Vec::new(),
        older: 0,
        gap: 0,
        version: None,
        reason: Some(reason),
    };
    let read = match trail_file(state, pane_id) {
        Ok(TrailFile::Claude(path)) => read(&state.trails, &path, cmux_core::trail::Trail::turns, turns, since_ms),
        Ok(TrailFile::Codex(known)) => {
            read(&state.codex_trails, &known.path, cmux_core::codex_trail::CodexTrail::turns, turns, since_ms)
        }
        Err(reason) => return empty(reason),
    };
    let Some((window, version)) = read else { return empty("no transcript found".into()) };
    cmux_protocol::AgentTrail {
        pane_id: pane_id.to_string(),
        turns: window.turns,
        older: window.older as u32,
        gap: window.gap as u32,
        version: Some(version),
        reason: None,
    }
}

/// One action of a pane's trail in full, read from its agent's file.
pub fn trail_detail(state: &AppState, pane_id: &str, action_id: &str) -> Result<cmux_protocol::ActionDetail, String> {
    match trail_file(state, pane_id)? {
        TrailFile::Claude(path) => cmux_core::trail::action_detail(&path, action_id),
        TrailFile::Codex(known) => cmux_core::codex_trail::action_detail(&known.path, action_id),
    }
    .ok_or_else(|| format!("no action {action_id} in this agent's trail"))
}

/// [`agent_trail`] for the board, off the main thread like the activity.
#[tauri::command]
pub async fn get_agent_trail(
    app: AppHandle,
    pane_id: String,
    turns: Option<u32>,
    since_ms: Option<u64>,
) -> Result<cmux_protocol::AgentTrail, String> {
    tauri::async_runtime::spawn_blocking(move || agent_trail(&app.state::<AppState>(), &pane_id, turns, since_ms))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_trail_detail(
    app: AppHandle,
    pane_id: String,
    action_id: String,
) -> Result<cmux_protocol::ActionDetail, String> {
    tauri::async_runtime::spawn_blocking(move || trail_detail(&app.state::<AppState>(), &pane_id, &action_id))
        .await
        .map_err(|e| e.to_string())?
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
pub(crate) fn broadcast_workspace(app: &AppHandle) {
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

/// Clears a pane's unread notifications without focusing it — the agent
/// wall, where clicking into an agent's terminal reads what it said and
/// leaves the tab's focused pane where it was.
#[tauri::command]
pub fn mark_pane_read(app: AppHandle, state: State<'_, AppState>, pane_id: String) {
    if notify::mark_pane_read(&state, &pane_id) {
        emit_workspace(&app);
    }
}

/// Shows `pane` alone, filling its tab (activating it), or — with
/// `zoom: false` — shows the whole tab again. With `zoom` absent it
/// toggles. The sidebar's agent entries zoom to their agent.
#[tauri::command]
pub fn zoom_pane(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    zoom: Option<bool>,
) -> Result<bool, String> {
    let zoomed = {
        let mut ws = state.workspace.lock().unwrap();
        let is_zoomed = ws
            .tabs
            .iter()
            .any(|t| t.zoomed.as_deref() == Some(pane_id.as_str()));
        if zoom.unwrap_or(!is_zoomed) {
            if !ws.zoom_pane(&pane_id) {
                return Err(format!("no pane {pane_id}"));
            }
            true
        } else {
            ws.unzoom(&pane_id);
            false
        }
    };
    if zoomed {
        notify::mark_pane_read(&state, &pane_id);
    }
    emit_workspace(&app);
    Ok(zoomed)
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

/// Shows the workspace as tabs or as the board.
#[tauri::command]
pub fn set_view(app: AppHandle, state: State<'_, AppState>, view: View) {
    if state.workspace.lock().unwrap().set_view(view) {
        emit_workspace(&app);
    }
}

/// Saved with the session but never broadcast: the board moving it shows
/// it already, and a snapshot per frame of a pan would re-render every
/// pane.
#[tauri::command]
pub fn set_camera(
    state: State<'_, AppState>,
    x: f64,
    y: f64,
    zoom: f64,
    width: Option<f64>,
    height: Option<f64>,
) -> Result<(), String> {
    let camera = cmux_core::board::camera(x, y, zoom)?;
    let mut ws = state.workspace.lock().unwrap();
    ws.camera = camera;
    if let (Some(w), Some(h)) = (width, height) {
        if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 {
            ws.viewport = Some((w, h));
        }
    }
    drop(ws);
    state
        .session_dirty
        .store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn set_card_rect(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<BoardRect, String> {
    let rect = cmux_core::board::card(x, y, w, h)?;
    let placed = state
        .workspace
        .lock()
        .unwrap()
        .set_card_rect(&pane_id, rect)
        .ok_or_else(|| format!("no pane {pane_id}"))?;
    // Refused or not: a drag that ended on a taken place snaps back to
    // what this snapshot says.
    emit_workspace(&app);
    Ok(placed)
}

#[tauri::command]
pub fn set_frame_position(
    app: AppHandle,
    state: State<'_, AppState>,
    tab_id: String,
    x: f64,
    y: f64,
) -> Result<BoardPoint, String> {
    let origin = cmux_core::board::frame_origin(x, y)?;
    let placed = state
        .workspace
        .lock()
        .unwrap()
        .set_frame_position(&tab_id, origin)
        .ok_or_else(|| format!("no tab {tab_id}"))?;
    emit_workspace(&app);
    Ok(placed)
}

/// The camera that shows the whole board in a `width` × `height` viewport.
/// Only answers: the board moves there itself and saves where it ends up.
#[tauri::command]
pub fn board_fit(state: State<'_, AppState>, width: f64, height: f64) -> Camera {
    let ws = state.workspace.lock().unwrap();
    cmux_core::board::fit(ws.tabs.iter().map(|t| t.frame_rect()), width, height)
}

/// Packs the frames into rows, in tab order, for a `width` × `height`
/// viewport (`Workspace::arrange_frames`), and answers the camera that
/// shows them all there — the board flies to it, as to Fit's.
#[tauri::command]
pub fn board_arrange(app: AppHandle, state: State<'_, AppState>, width: f64, height: f64) -> Camera {
    let camera = {
        let mut ws = state.workspace.lock().unwrap();
        ws.arrange_frames(width, height);
        cmux_core::board::fit(ws.tabs.iter().map(|t| t.frame_rect()), width, height)
    };
    emit_workspace(&app);
    camera
}

/// The camera that shows `pane_id`'s card centred in a `width` × `height`
/// viewport: at 100% — where double-clicking its header goes — or at
/// `zoom`, to bring it into view as the board is.
#[tauri::command]
pub fn board_card_camera(
    state: State<'_, AppState>,
    pane_id: String,
    width: f64,
    height: f64,
    zoom: Option<f64>,
) -> Result<Camera, String> {
    let ws = state.workspace.lock().unwrap();
    let tab = ws.tab_of_pane(&pane_id).ok_or_else(|| format!("no pane {pane_id}"))?;
    let card = tab.cards.get(&pane_id).ok_or_else(|| format!("no card for {pane_id}"))?;
    let on_board = BoardRect { x: tab.frame.x + card.x, y: tab.frame.y + card.y, ..*card };
    let zoom = zoom.unwrap_or(1.0).clamp(cmux_core::board::MIN_ZOOM, cmux_core::board::MAX_ZOOM);
    Ok(cmux_core::board::centre_on(on_board, width, height, zoom))
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

    // A factory: a held pane that fails to attach falls back to a local
    // one, and each attempt consumes its own sink.
    let make_sink = || {
        let on_data = on_data.clone();
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
        state.pty.set_sink(&pane_id, make_sink());
        let _ = state.pty.resize(&pane_id, cols, rows);
        return Ok("reattached".into());
    }

    // A pane whose holder outlived the last quit: its process never
    // stopped, so it is reattached rather than restored — and none of the
    // idle-until-keypress rules apply, because nothing is being started.
    if state.pending_reattach.lock().unwrap().remove(&pane_id) {
        let socket = cmux_core::hold::socket_path(&pane_id);
        match state.pty.attach_held(
            &pane_id,
            &socket,
            cols,
            rows,
            pane_scanner(app, &pane_id),
            make_sink(),
            pane_exit_hook(app),
        ) {
            Ok((missed, last)) => {
                summarize_missed(app, &pane_id, missed, last);
                return Ok("resumed".into());
            }
            // It died since launch: open the pane as if it had none.
            Err(e) => eprintln!("mirador: reattaching {pane_id}: {e}"),
        }
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

    let program = match (remote_host.as_deref(), pane_command.as_deref()) {
        (Some(host), _) => Program::Ssh(host.to_string()),
        (None, Some(cmd)) => Program::Command(cmd.to_string()),
        (None, None) => Program::Shell,
    };
    if spawn_held(app, &pane_id, cwd.as_deref(), cols, rows, &program, &make_sink) {
        return Ok("spawned".into());
    }

    state.pty.spawn(
        &pane_id,
        cols,
        rows,
        cwd.as_deref(),
        command_builder,
        pane_scanner(app, &pane_id),
        make_sink(),
        pane_exit_hook(app),
    )?;

    Ok("spawned".into())
}

/// The OSC scanner every terminal pane's output goes through. Its events
/// run on the pane's reader thread.
fn pane_scanner(app: &AppHandle, pane_id: &str) -> Box<dyn cmux_core::osc::StreamScanner> {
let app = app.clone();
let pane_id = pane_id.to_string();
    Box::new(OscScanner::new(move |event| match event {
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
    }))
}

/// What a terminal pane does when its process exits, wherever it ran.
fn pane_exit_hook(app: &AppHandle) -> impl FnOnce(&str, Option<i32>) + Send + 'static {
    let exit_app = app.clone();
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
    }
}

/// Where a terminal pane's process runs.
enum Program {
    Shell,
    Command(String),
    Ssh(String),
}

/// With `persistSessions` on, runs the pane in a session holder so it
/// outlives the app. Returns false — and the caller spawns locally — when
/// it is off, or anything about the holder fails: a broken holder must
/// never cost the user a terminal.
fn spawn_held<S: FnMut(&[u8]) + Send + 'static>(
    app: &AppHandle,
    pane_id: &str,
    cwd: Option<&str>,
    cols: u16,
    rows: u16,
    program: &Program,
    make_sink: &impl Fn() -> S,
) -> bool {
    use cmux_core::hold::server::Program as Held;
    if !cmux_core::config::persist_sessions() {
        return false;
    }
    let Some(mira) = mira_binary() else {
        eprintln!("mirador: persistSessions is on but the mira binary is missing");
        return false;
    };
    let socket = cmux_core::hold::socket_path(pane_id);
    let held = match program {
        Program::Shell => Held::Shell,
        Program::Command(cmd) => Held::Command(cmd.clone()),
        Program::Ssh(host) => Held::Ssh(host.clone()),
    };
    let state = app.state::<AppState>();
    let result = cmux_core::hold::launch(&mira, pane_id, &socket, cwd, cols, rows, &held)
        .map_err(|e| e.to_string())
        .and_then(|()| {
            // A holder this new has had no time to miss anything.
            state.pty.attach_held(
                pane_id,
                &socket,
                cols,
                rows,
                pane_scanner(app, pane_id),
                make_sink(),
                pane_exit_hook(app),
            )
        })
        .map(|_| ());
    match result {
        Ok(()) => true,
        Err(e) => {
            eprintln!("mirador: session holder for {pane_id} failed, running it locally: {e}");
            false
        }
    }
}

/// One notification for everything a held pane raised while Mirador was
/// closed: replaying them one by one would be noise, and dropping them
/// would hide a finished agent.
fn summarize_missed(app: &AppHandle, pane_id: &str, count: u32, last: Option<String>) {
    if count == 0 {
        return;
    }
    let last = last.unwrap_or_default();
    // Said in the body too: an agent's notification is titled with the
    // agent and its tab, and that title replaces this one.
    let body = if count == 1 {
        format!("While Mirador was closed: {last}")
    } else {
        format!("While Mirador was closed: {count} notifications — the last: {last}")
    };
    notify::handle_notification(app, pane_id, Some("While Mirador was closed".into()), body);
}

/// Whether to offer the Claude Code integration: Claude Code is used here,
/// none of our hooks are installed, nobody has answered the offer, and this
/// build has a `mira` for the hooks to call.
#[tauri::command]
pub fn claude_integration_offer() -> bool {
    use cmux_core::claude_integration as ci;
    mira_binary().is_some() && ci::should_offer(&ci::status(), ci::offer_answered())
}

/// Sets the integration up: `mira` on PATH (agents and `/mira-diff` call it
/// by name), then the hooks and the skill (which call it by path). Answers
/// the offer either way, so a failure is not offered again unasked — the
/// palette entry is how to retry. Returns what was done, line by line.
#[tauri::command]
pub async fn setup_claude_integration() -> Result<Vec<String>, String> {
    use cmux_core::claude_integration as ci;
    tauri::async_runtime::spawn_blocking(|| {
        ci::mark_offer_answered();
        let mira = mira_binary().ok_or("this build has no mira binary beside it")?;
        let mut done = ci::install_on_path(&mira)?;
        done.extend(ci::hooks(ci::Action::Setup, &mira)?);
        Ok(done)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// "Not now": the offer is not made again. The palette entry remains.
#[tauri::command]
pub fn dismiss_claude_integration() {
    cmux_core::claude_integration::mark_offer_answered();
}

/// Points hooks at this `mira` again when the one they call is gone — the
/// app was moved, or reinstalled elsewhere. Only then: a hook calling some
/// other `mira` that still exists (a release build, while this is a dev
/// one) is left alone, and so is the older form that calls `mira` by name.
/// Repairing is not a new decision — the hooks are already ours.
///
/// Hooks from a version that installed fewer events get the rest, calling
/// the `mira` they already call when it still exists: the agent wall's
/// status needs `UserPromptSubmit` and `PostToolUse`, and an install from
/// before it has neither.
pub fn repair_claude_hooks() {
    use cmux_core::claude_integration as ci;
    let mira = match ci::status().hooks {
        ci::HookState::Broken => mira_binary(),
        ci::HookState::Outdated => ci::installed_mira().or_else(mira_binary),
        ci::HookState::Missing | ci::HookState::Installed => return,
    };
    let Some(mira) = mira else { return };
    match ci::hooks(ci::Action::Setup, &mira) {
        Ok(_) => eprintln!("mirador: Claude Code hooks updated to call {}", mira.display()),
        Err(e) => eprintln!("mirador: could not repair the Claude Code hooks: {e}"),
    }
}

/// Ends every terminal session — including ones still waiting in their
/// holders — then quits. Cmd+Q leaves them running; this is the way to
/// mean "everything".
#[tauri::command]
pub async fn quit_ending_sessions(app: AppHandle) {
    let _ = tauri::async_runtime::spawn_blocking(|| {
        let left = cmux_core::hold::end_all(std::time::Duration::from_secs(3));
        if !left.is_empty() {
            eprintln!("mirador: holders still running at quit: {left:?}");
        }
    })
    .await;
    app.exit(0);
}

/// The bundled `mira` CLI, which doubles as the session holder. It sits
/// next to the app binary — in the bundle, and in `target/<profile>` when
/// the workspace has been built.
pub(crate) fn mira_binary() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) { "mira.exe" } else { "mira" };
    let path = std::env::current_exe().ok()?.parent()?.join(name);
    path.is_file().then_some(path)
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
    // Before the pane exists: a URL that does not parse opens no pane.
    let url = crate::browser::parse_url(&app, &url, None)?.to_string();
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

/// Starts a Claude Code agent: a shell pane (a split of `pane_id` — the
/// usual way, so the agent works in the tab beside you — or a new tab)
/// with `claude` typed into it, carrying the role's model and prompt and
/// `task` as its first message. A shell rather than a command pane, as a
/// restored agent is: `/exit` leaves a prompt, and the session it records
/// resumes like any other.
// Off the main thread: a role that keeps a list of skills or plugins
// asks Claude Code for its inventory first, which takes a second.
#[tauri::command(async)]
pub fn open_agent(
    app: AppHandle,
    state: State<'_, AppState>,
    role: Option<String>,
    task: Option<String>,
    pane_id: Option<String>,
    tab: bool,
    harness: Option<cmux_protocol::Harness>,
) -> Result<String, String> {
    open_agent_from(app, state, role, task, pane_id, tab, harness, None, "you".into())
}

/// [`open_agent`], its task logged as given by `from`: a person, from the
/// app; whoever ran `mira agent new`, from the socket. `harness` is for an
/// agent without a role: Claude Code, else Codex. `title` is the task's
/// card's, instead of one made from the task.
#[allow(clippy::too_many_arguments)]
pub fn open_agent_from(
    app: AppHandle,
    state: State<'_, AppState>,
    role: Option<String>,
    task: Option<String>,
    pane_id: Option<String>,
    tab: bool,
    harness: Option<cmux_protocol::Harness>,
    title: Option<String>,
    from: String,
) -> Result<String, String> {
    if role.is_some() && harness.is_some() {
        return Err("a role names its own harness: give a role or a harness, not both".into());
    }
    let plain = cmux_core::agents::plain_harness(harness)?;
    let source = pane_id
        .filter(|id| state.meta.lock().unwrap().contains_key(id))
        .unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    // The agent works where it was asked from, whichever way it opens, and
    // gets the roles that directory sees: its repository's, then yours.
    let cwd = state
        .meta
        .lock()
        .unwrap()
        .get(&source)
        .and_then(|m| m.cwd.clone())
        .or_else(cmux_core::config::default_cwd);
    let launch = prepare_launch(role.as_deref(), plain, None, cwd.as_deref(), None, None, task.as_deref())?;

    let new_pane = if tab {
        state.workspace.lock().unwrap().new_tab().1
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
        mark_launched(meta.entry(new_pane.clone()).or_default(), launch, cwd);
    }
    emit_workspace(&app);
    if let Some(task) = &task {
        crate::tasks::given(&app, cmux_protocol::TaskSource::AgentNew, from, &new_pane, task, None, title);
    }
    Ok(new_pane)
}

/// An agent ready to be typed into a pane: the command line, and what to
/// remember about it on the pane.
pub(crate) struct Launch {
    pub input: String,
    /// What it runs: the role's harness, or the plain one.
    pub harness: cmux_protocol::Harness,
    pub role: Option<cmux_protocol::AgentRole>,
    pub settings: Option<String>,
}

/// Works out how to start an agent in `cwd`: plain Claude Code, or `role`
/// (looked up in `catalog`, else in what `cwd` sees) with its prompt and
/// limits; without a role, `plain` (`agents::plain_harness`). `extra`
/// follows the role's prompt — a team member's roster —
/// and `name` is the session's name instead of the role's. Every file it
/// needs is written; nothing is opened, so a refusal changes nothing.
pub(crate) fn prepare_launch(
    role: Option<&str>,
    plain: cmux_protocol::Harness,
    catalog: Option<&cmux_protocol::RoleCatalog>,
    cwd: Option<&str>,
    extra: Option<&str>,
    name: Option<&str>,
    task: Option<&str>,
) -> Result<Launch, String> {
    let (role, prompt) = match role.map(str::trim).filter(|r| !r.is_empty()) {
        Some(role) => {
            let read;
            let catalog = match catalog {
                Some(c) => c,
                None => {
                    read = cmux_core::roles::catalog(cwd.map(std::path::Path::new));
                    &read
                }
            };
            let (role, prompt) = cmux_core::roles::resolve(catalog, role)?;
            (Some(role), prompt)
        }
        None => (None, None),
    };
    let prompt = [prompt.as_deref(), extra]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let prompt = (!prompt.is_empty()).then_some(prompt);
    // What a Claude Code role may use: its MCP servers, plugins, skills and
    // tools, as a settings file. A list of skills or plugins to keep needs
    // every name there is, which only Claude Code knows — asked in the
    // directory the agent will work in, on the PATH its shell would give
    // it. Nothing starts if that fails: an agent meant to be limited must
    // not start with everything.
    let settings_file = match &role {
        Some(r) if cmux_core::role_limits::limits(r) => {
            let inventory = if cmux_core::role_limits::needs_inventory(r) {
                #[cfg(unix)]
                let path = cmux_core::pty::shell::interactive_path();
                #[cfg(not(unix))]
                let path: Option<&str> = None;
                Some(
                    cmux_core::role_limits::probe(cwd.map(std::path::Path::new), path)
                        .map_err(|e| format!("role `{}`: {e}", r.name))?,
                )
            } else {
                None
            };
            let settings = cmux_core::role_limits::settings(r, inventory.as_ref())
                .map_err(|e| format!("role `{}`: {e}", r.name))?;
            Some(
                cmux_core::roles::write_settings(&r.name, &settings)
                    .map_err(|e| format!("could not write the role's settings: {e}"))?,
            )
        }
        _ => None,
    };
    // Claude Code reads a role's prompt from a file; Codex takes it inline.
    // Written once the role is known to start, so a refused one leaves none.
    let prompt_file = match (&role, &prompt) {
        (Some(r), Some(p)) if r.harness == cmux_protocol::Harness::Claude => Some(
            cmux_core::roles::write_prompt(name.unwrap_or(&r.name), p)
                .map_err(|e| format!("could not write the role's prompt: {e}"))?,
        ),
        _ => None,
    };
    // The session goes by the member's name; the pane keeps the role's, to
    // find it again on a restore.
    let named = role.clone().map(|r| cmux_protocol::AgentRole { name: name.unwrap_or(&r.name).to_string(), ..r });
    // Codex is allowed Mirador's socket on top of the profile it would run
    // on anyway — on Unix: Windows has a named pipe, and no such sandbox.
    let harness = role.as_ref().map_or(plain, |r| r.harness);
    let codex_socket = if cfg!(unix) && harness == cmux_protocol::Harness::Codex {
        let home = std::env::var_os("CODEX_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".codex")));
        // No file is no config; one that cannot be read is not guessed at.
        let config = match home.as_ref().map(|h| std::fs::read_to_string(h.join("config.toml"))) {
            Some(Ok(text)) => Some(Some(text)),
            Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => None,
            _ => Some(None),
        };
        config
            .and_then(|c| {
                // Codex 0.164 keeps a profile in its own file beside
                // config.toml; a missing one is Codex's to report.
                let profile = role.as_ref().and_then(|r| r.profile.as_ref()).and_then(|p| {
                    let file = home.as_ref()?.join(format!("{p}.config.toml"));
                    std::fs::read_to_string(file).ok()
                });
                cmux_core::agents::codex_base_profile(c.as_deref(), cwd.map(std::path::Path::new), profile.as_deref())
            })
            .map(|base| (base, cmux_core::ipc::default_socket_path()))
    } else {
        None
    };
    let input = cmux_core::agents::launch_command(
        named.as_ref(),
        plain,
        prompt.as_deref(),
        prompt_file.as_deref(),
        settings_file.as_deref(),
        task,
        codex_socket.as_ref().map(|(base, socket)| (base.as_str(), socket.as_path())),
        cmux_core::pty::shell::quote_arg,
    );
    Ok(Launch {
        input,
        harness,
        role,
        settings: settings_file.map(|f| f.to_string_lossy().into_owned()),
    })
}

/// Marks `entry` as the pane `launch` starts in: typed into its shell once
/// it first prints, and an agent from now on.
pub(crate) fn mark_launched(entry: &mut cmux_core::state::PaneMeta, launch: Launch, cwd: Option<String>) {
    entry.cwd = cwd;
    entry.startup_input = Some(launch.input);
    entry.agent_launched = true;
    entry.agent_started_at = Some(std::time::Instant::now());
    // A Codex is a Codex agent from the start, not a Claude one until the
    // poller sees otherwise — and on Windows, which lists no processes, the
    // poller never would.
    if launch.harness != cmux_protocol::Harness::Claude {
        entry.agent_harness = Some(launch.harness);
    }
    if let Some(role) = launch.role {
        entry.agent_model = role.model;
        entry.agent_effort = role.effort;
        entry.agent_role = Some(role.name);
        entry.agent_settings = launch.settings;
    }
}

/// The roles and ways of working `pane_id`'s directory sees (the focused
/// pane's when absent): its repository's `.mirador`, then the user's.
#[tauri::command]
pub fn agent_roles(state: State<'_, AppState>, pane_id: Option<String>) -> cmux_protocol::RoleCatalog {
    let pane = pane_id.unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    let cwd = state
        .meta
        .lock()
        .unwrap()
        .get(&pane)
        .and_then(|m| m.cwd.clone())
        .or_else(cmux_core::config::default_cwd);
    cmux_core::roles::catalog(cwd.as_deref().map(std::path::Path::new))
}

/// Shows the tab of `pane_id` (the focused pane when absent) as its wall's
/// agents in the `mode` layout, turning the wall on if it is off.
pub fn show_wall_in(
    app: &AppHandle,
    state: &AppState,
    pane_id: Option<String>,
    mode: cmux_protocol::WallMode,
) -> Result<(), String> {
    let pane = pane_id
        .filter(|id| state.meta.lock().unwrap().contains_key(id))
        .unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    if !state.workspace.lock().unwrap().show_wall_in(&pane, mode) {
        return Err(format!("no pane {pane}"));
    }
    emit_workspace(app);
    Ok(())
}

/// The layout of the wall of `pane_id`'s tab (the focused pane's when
/// absent).
pub fn wall_mode(state: &AppState, pane_id: Option<&str>) -> cmux_protocol::WallMode {
    let ws = state.workspace.lock().unwrap();
    let pane = pane_id.map(String::from).unwrap_or_else(|| ws.focused_pane());
    ws.tab_of_pane(&pane).map(|t| t.wall_mode).unwrap_or_default()
}

/// Shows the tab of `pane_id` (the focused pane when absent) as the wall
/// of its agents, or its splits again when it already is. The new state.
#[tauri::command]
pub fn toggle_agent_wall(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: Option<String>,
) -> Result<bool, String> {
    let pane = pane_id
        .filter(|id| state.meta.lock().unwrap().contains_key(id))
        .unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    let on = state
        .workspace
        .lock()
        .unwrap()
        .toggle_wall(&pane)
        .ok_or_else(|| format!("no pane {pane}"))?;
    emit_workspace(&app);
    Ok(on)
}

/// Shows the tab of `pane_id` (the focused pane when absent) as its wall,
/// on the `view` side: its agents, or its task board.
#[tauri::command]
pub fn set_wall_view(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: Option<String>,
    view: cmux_protocol::WallView,
) -> Result<(), String> {
    let pane = pane_id
        .filter(|id| state.meta.lock().unwrap().contains_key(id))
        .unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    if !state.workspace.lock().unwrap().show_wall(&pane, view) {
        return Err(format!("no pane {pane}"));
    }
    emit_workspace(&app);
    Ok(())
}

/// Lays the Agents side of the wall of `pane_id`'s tab (the focused pane's
/// when absent) out as `mode`, without turning the wall on.
#[tauri::command]
pub fn set_wall_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: Option<String>,
    mode: cmux_protocol::WallMode,
) -> Result<(), String> {
    let pane = pane_id
        .filter(|id| state.meta.lock().unwrap().contains_key(id))
        .unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    if !state.workspace.lock().unwrap().set_wall_mode(&pane, mode) {
        return Err(format!("no pane {pane}"));
    }
    emit_workspace(&app);
    Ok(())
}

/// Lays the Tasks side of the wall of `pane_id`'s tab (the focused pane's
/// when absent) out as `layout`, without turning the wall on.
#[tauri::command]
pub fn set_task_layout(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: Option<String>,
    layout: cmux_protocol::TaskLayout,
) -> Result<(), String> {
    let pane = pane_id
        .filter(|id| state.meta.lock().unwrap().contains_key(id))
        .unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    if !state.workspace.lock().unwrap().set_task_layout(&pane, layout) {
        return Err(format!("no pane {pane}"));
    }
    emit_workspace(&app);
    Ok(())
}

/// Sets how many agents a page of the wall's Grid holds: 4, 6 or 9, in
/// mirador.json. The config watcher picks the file up like any edit, and
/// a new snapshot refills the pages.
#[tauri::command]
pub fn set_agents_per_tab(n: u8) -> Result<(), String> {
    cmux_core::config::set_agents_per_tab(n)
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
///
/// Async, so off the main thread: a sync command runs there, inside the
/// host webview's IPC handler, and on Windows WebView2 cannot create a
/// webview from in there (tauri-apps/wry#583). `add_child` waited forever
/// for a page that never came, and every IPC call queued behind it — the
/// board still panned, being the frontend's alone, but no button or
/// palette action reached Rust again.
#[tauri::command]
pub async fn set_browser_bounds(
    app: AppHandle,
    pane_id: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), String> {
    crate::browser::ensure_webview(&app, &pane_id, x, y, w, h)
}

/// The pane's page as PNG bytes, for its card on the board to show while
/// the page itself is hidden. On a blocking thread: the capture waits on
/// the main thread, which takes the snapshot.
#[tauri::command]
pub async fn browser_screenshot(app: AppHandle, pane_id: String) -> Result<tauri::ipc::Response, String> {
    let png = tauri::async_runtime::spawn_blocking(move || crate::browser::screenshot(&app, &pane_id))
        .await
        .map_err(|e| e.to_string())??;
    Ok(tauri::ipc::Response::new(png))
}

#[tauri::command]
pub fn set_browser_visible(app: AppHandle, pane_id: String, visible: bool) {
    crate::browser::set_visible(&app, &pane_id, visible);
}

/// Async: off the main thread, which `browser::webview` may wait on.
#[tauri::command]
pub async fn browser_navigate(app: AppHandle, pane_id: String, url: String) -> Result<(), String> {
    // The URL bar does not wait for the page: the person watches it load.
    crate::browser::navigate(&app, &pane_id, &url).map(|_| ())
}

/// Async: off the main thread, which `browser::webview` may wait on.
#[tauri::command]
pub async fn browser_history(app: AppHandle, pane_id: String, action: String) -> Result<(), String> {
    crate::browser::history(&app, &pane_id, &action).map(|_| ())
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
pub async fn load_scrollback(state: State<'_, AppState>, pane_id: String) -> Result<Option<String>, ()> {
    // A pane reattaching to its holder gets the holder's replay, which is
    // newer than any saved scrollback; showing both would print the same
    // history twice.
    if state.held_at_launch.contains(&pane_id) {
        return Ok(None);
    }
    Ok(cmux_core::session::load_scrollback(&pane_id))
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
    let mut agents = Vec::new();
    for pane in panes {
        let _ = state.pty.close(pane);
        if meta.remove(pane).is_some_and(|m| m.is_agent()) {
            agents.push(pane.clone());
        }
    }
    drop(meta);
    for pane in agents {
        let end = cmux_protocol::TurnEnd::Exited;
        crate::tasks::record(app, cmux_protocol::TaskEventKind::TurnEnded { pane, end });
    }
    notify::forget_panes(state, panes);
    state.trail_files.lock().unwrap().retain(|pane, _| !panes.contains(pane));
    for pane in panes {
        crate::browser::destroy(app, pane);
        runs::forget_pane(state, pane);
        cmux_core::session::delete_scrollback(pane);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(role: Option<cmux_protocol::AgentRole>) -> Launch {
        Launch { input: "claude\n".into(), harness: cmux_protocol::Harness::Claude, role, settings: None }
    }

    #[test]
    fn a_launched_agent_shows_its_roles_model_and_effort() {
        let role = cmux_protocol::AgentRole {
            name: "planner".into(),
            model: Some("opus".into()),
            effort: Some("high".into()),
            ..Default::default()
        };
        let mut m = cmux_core::state::PaneMeta::default();
        mark_launched(&mut m, launch(Some(role)), None);
        assert_eq!(m.agent_model.as_deref(), Some("opus"));
        assert_eq!(m.agent_effort.as_deref(), Some("high"));

        // A role that sets no effort, and no role at all, show none.
        let mut m = cmux_core::state::PaneMeta::default();
        let plain = cmux_protocol::AgentRole { name: "dev".into(), ..Default::default() };
        mark_launched(&mut m, launch(Some(plain)), None);
        assert_eq!(m.agent_effort, None);
        let mut m = cmux_core::state::PaneMeta::default();
        mark_launched(&mut m, launch(None), None);
        assert_eq!(m.agent_effort, None);
    }
}
