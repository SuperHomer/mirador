//! Checkpoints in the app: saving a team's conversations, or one agent's,
//! from the panes running them, and resuming them in new ones. What a
//! checkpoint holds, where it lives and what is refused are
//! `cmux_core::checkpoints`'.

use tauri::Manager;

use cmux_protocol::{Checkpoint, CheckpointMember, Harness, SplitDir, TeamRun, TeamSeat};

use crate::commands::{emit_workspace, mark_launched, prepare_launch};
use crate::AppState;

/// The conversation the agent in `pane` is on, as a member called `name`.
fn member_of(state: &AppState, pane: &str, name: &str, role: Option<String>, lead: bool) -> Result<CheckpointMember, String> {
    let (harness, record, pid, cwd) = {
        let meta = state.meta.lock().unwrap();
        let m = meta.get(pane).filter(|m| m.is_agent()).ok_or("its agent is not running")?;
        (m.harness(), m.agent_session.clone(), m.agent_pid, m.cwd.clone())
    };
    let session = match harness {
        Some(Harness::Claude) => cmux_core::checkpoints::claude_session(record.as_deref())?,
        Some(Harness::Codex) => cmux_core::checkpoints::codex_session(pid.ok_or("its Codex is not running")?)?,
        Some(other) => return Err(format!("{} cannot be resumed by Mirador", other.label())),
        None => return Err("its harness is not known".into()),
    };
    Ok(CheckpointMember { name: name.to_string(), role, harness: harness.unwrap_or_default(), lead, cwd, session })
}

/// Saves the team of `pane_id`'s tab as `name`, its team's name by default.
/// Nothing is written unless every member's conversation can be read.
pub fn team_save(state: &AppState, name: Option<String>, pane_id: Option<String>) -> Result<Checkpoint, String> {
    let (tab, run) = crate::teams::team_here(state, pane_id.as_deref())?;
    let in_tab: Vec<String> = {
        let ws = state.workspace.lock().unwrap();
        ws.tabs.iter().find(|t| t.id == tab).map(|t| cmux_core::layout::pane_ids(&t.root)).unwrap_or_default()
    };
    let mut members = Vec::new();
    let mut problems = Vec::new();
    for seat in &run.members {
        let member = if in_tab.contains(&seat.pane) {
            member_of(state, &seat.pane, &seat.name, Some(seat.role.clone()), seat.lead)
        } else {
            Err("its pane was closed".into())
        };
        match member {
            Ok(m) => members.push(m),
            Err(e) => problems.push(format!("{}: {e}", seat.name)),
        }
    }
    if !problems.is_empty() {
        return Err(format!("nothing saved — {}", problems.join("; ")));
    }
    let catalog = cmux_core::roles::catalog(crate::teams::cwd_of(state, pane_id.as_deref()).as_deref().map(std::path::Path::new));
    let checkpoint = Checkpoint {
        name: name.unwrap_or_else(|| run.name.clone()),
        saved_at: crate::runs::now_ms(),
        layout: catalog.teams.iter().find(|t| t.name == run.name).and_then(|t| t.layout.clone()),
        team: Some(run.name),
        cwd: crate::teams::cwd_of(state, pane_id.as_deref()),
        members,
    };
    cmux_core::checkpoints::save(&cmux_core::checkpoints::saves_dir(), &checkpoint)?;
    Ok(checkpoint)
}

/// Saves the agent `name` means from the caller's tab (as `agent send`
/// finds it), or the caller's own, as `save_as` — by default what it goes
/// by: its member name, its session's, its role's.
pub fn agent_save(
    state: &AppState,
    name: Option<&str>,
    save_as: Option<String>,
    caller: Option<String>,
) -> Result<Checkpoint, String> {
    let pane = match name {
        Some(name) => crate::handoff::find(state, name, caller)?,
        None => caller.unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane()),
    };
    let label = name.unwrap_or(&pane).to_string();
    let role = state.meta.lock().unwrap().get(&pane).and_then(|m| m.agent_role.clone());
    let called = crate::handoff::sender(state, &pane).unwrap_or_else(|| "agent".into());
    let member = member_of(state, &pane, &called, role, false).map_err(|e| format!("nothing saved — {label}: {e}"))?;
    let save_as = save_as.unwrap_or_else(|| called.clone());
    if !cmux_core::roles::is_name(&save_as) {
        return Err(format!("`{save_as}` cannot name a save: give one with --as"));
    }
    let checkpoint = Checkpoint {
        name: save_as,
        saved_at: crate::runs::now_ms(),
        cwd: member.cwd.clone(),
        members: vec![member],
        ..Default::default()
    };
    cmux_core::checkpoints::save(&cmux_core::checkpoints::saves_dir(), &checkpoint)?;
    Ok(checkpoint)
}

/// Every save, newest first.
pub fn list() -> Vec<Checkpoint> {
    cmux_core::checkpoints::list(&cmux_core::checkpoints::saves_dir())
}

/// Refuses `checkpoint` while a pane here runs one of its conversations:
/// two processes on one conversation write over each other.
fn not_running(state: &AppState, checkpoint: &Checkpoint) -> Result<(), String> {
    let wants_codex = checkpoint.members.iter().any(|m| m.harness == Harness::Codex);
    // Each pane's Claude session, or its Codex's pid: finding a Codex's
    // conversation runs `lsof`, which is not done holding the lock.
    let agents: Vec<(String, Option<String>, Option<u32>)> = {
        let meta = state.meta.lock().unwrap();
        meta.iter()
            .filter(|(_, m)| m.is_agent())
            .map(|(p, m)| match m.harness() {
                Some(Harness::Claude) => {
                    (p.clone(), cmux_core::checkpoints::claude_session(m.agent_session.as_deref()).ok(), None)
                }
                Some(Harness::Codex) if wants_codex => (p.clone(), None, m.agent_pid),
                _ => (p.clone(), None, None),
            })
            .collect()
    };
    let mut running = Vec::new();
    for (pane, claude, codex_pid) in agents {
        let id = claude.or_else(|| codex_pid.and_then(|p| cmux_core::checkpoints::codex_session(p).ok()));
        if let Some(m) = id.and_then(|id| checkpoint.members.iter().find(|m| m.session == id)) {
            let who = crate::handoff::sender(state, &pane).map(|n| format!("{n}, ")).unwrap_or_default();
            running.push(format!("{}'s in pane {who}{pane}", m.name));
        }
    }
    if running.is_empty() {
        return Ok(());
    }
    running.sort();
    Err(format!("its conversations are still running — {}: quit them there first", running.join("; ")))
}

/// What to type to resume `member`: its launch line — its role's flags,
/// and `extra` after its prompt — with the resume in front.
fn resume_launch(
    member: &CheckpointMember,
    catalog: &cmux_protocol::RoleCatalog,
    cwd: Option<&str>,
    extra: &[cmux_protocol::PromptLayer],
) -> Result<crate::commands::Launch, String> {
    let mut launch = prepare_launch(member.role.as_deref(), member.harness, Some(catalog), cwd, extra, Some(&member.name), None)
        .map_err(|e| format!("member `{}`: {e}", member.name))?;
    launch.input = cmux_core::checkpoints::resume_command(member, &launch.input)?;
    Ok(launch)
}

/// Opens the team save `name` in a tab of its own, as `team start` lays a
/// team out, each member resuming its conversation where it was saved.
/// Returns the tab's id.
pub fn team_resume(app: &tauri::AppHandle, name: &str) -> Result<String, String> {
    let state = &*app.state::<AppState>();
    let checkpoint = cmux_core::checkpoints::load(&cmux_core::checkpoints::saves_dir(), name)?;
    let Some(team) = checkpoint.team.clone() else {
        return Err(format!("`{name}` is one agent's save: `mira agent resume {name}` opens it"));
    };
    not_running(state, &checkpoint)?;
    let panes = cmux_core::state::Workspace::fresh_pane_ids(checkpoint.members.len());
    let mut members = Vec::new();
    for (m, pane) in checkpoint.members.iter().zip(&panes) {
        let role = m.role.clone().ok_or_else(|| format!("member `{}` has no role", m.name))?;
        members.push(TeamSeat { name: m.name.clone(), role, pane: pane.clone(), lead: m.lead });
    }
    let run = TeamRun { name: team.clone(), members };
    let catalog = cmux_core::roles::catalog(checkpoint.cwd.as_deref().map(std::path::Path::new));
    let mut launches = Vec::new();
    for (m, seat) in checkpoint.members.iter().zip(&run.members) {
        let me = cmux_core::teams::Seat { name: seat.name.clone(), role: seat.role.clone(), lead: seat.lead };
        let roster = crate::teams::roster_now(&run, &catalog, &me, None);
        let cwd = m.cwd.as_deref().or(checkpoint.cwd.as_deref());
        launches.push((resume_launch(m, &catalog, cwd, &roster)?, cwd.map(str::to_string)));
    }

    let root = cmux_core::teams::layout(&panes, checkpoint.layout.as_deref());
    let tab = state.workspace.lock().unwrap().new_team_tab(root, run, &team);
    {
        let mut meta = state.meta.lock().unwrap();
        for (pane, (launch, cwd)) in panes.iter().zip(launches) {
            mark_launched(meta.entry(pane.clone()).or_default(), launch, cwd);
        }
    }
    emit_workspace(app);
    Ok(tab)
}

/// Opens one agent's save `name` beside `pane_id` (the focused pane), or
/// in a new tab, resuming its conversation. Returns its pane.
pub fn agent_resume(app: &tauri::AppHandle, name: &str, pane_id: Option<String>, tab: bool) -> Result<String, String> {
    let state = &*app.state::<AppState>();
    let checkpoint = cmux_core::checkpoints::load(&cmux_core::checkpoints::saves_dir(), name)?;
    if checkpoint.team.is_some() {
        return Err(format!("`{name}` is a team's save: `mira team resume {name}` opens it"));
    }
    let [member] = checkpoint.members.as_slice() else {
        return Err(format!("save `{name}` holds more than one agent"));
    };
    not_running(state, &checkpoint)?;
    let cwd = member.cwd.as_deref().or(checkpoint.cwd.as_deref());
    let catalog = cmux_core::roles::catalog(cwd.map(std::path::Path::new));
    let launch = resume_launch(member, &catalog, cwd, &[])?;
    let pane = if tab {
        state.workspace.lock().unwrap().new_tab().1
    } else {
        let source = pane_id
            .filter(|id| state.meta.lock().unwrap().contains_key(id))
            .unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
        state.workspace.lock().unwrap().split_pane(&source, SplitDir::Row).ok_or_else(|| format!("no pane {source}"))?
    };
    mark_launched(state.meta.lock().unwrap().entry(pane.clone()).or_default(), launch, cwd.map(str::to_string));
    emit_workspace(app);
    Ok(pane)
}
