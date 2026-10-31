//! Teams in the app: starting one in a tab of its own, and keeping track
//! of it there — listing its members, adding one, restarting those whose
//! agent exited or whose pane was closed, clearing them. The team's definition and the
//! rules for it are in `cmux_core::teams`; this is where it meets panes.

use tauri::{AppHandle, State};

use cmux_protocol::{SeatState, SplitDir, TeamInfo, TeamMemberInfo, TeamRun, TeamSeat};

use crate::commands::{emit_workspace, mark_launched, prepare_launch};
use crate::AppState;

/// The directory `pane_id` (the focused pane when absent) works in.
pub(crate) fn cwd_of(state: &AppState, pane_id: Option<&str>) -> Option<String> {
    let pane = pane_id
        .map(str::to_string)
        .unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    state
        .meta
        .lock()
        .unwrap()
        .get(&pane)
        .and_then(|m| m.cwd.clone())
        .or_else(cmux_core::config::default_cwd)
}

/// How `mira` is called in a member's prompt: by its absolute path, since
/// the agent's shell may not have it on its PATH.
pub(crate) fn mira_for_prompt() -> String {
    crate::commands::mira_binary()
        .map(|p| cmux_core::pty::shell::quote_arg(&p.to_string_lossy()))
        .unwrap_or_else(|| "mira".into())
}

/// Starts `team` — the one `pane_id`'s directory sees — in a tab of its
/// own, each member told its teammates; `task` goes to the lead. Every
/// member's launch is worked out before any pane opens, so a team that
/// cannot start opens nothing. Returns the tab's id.
#[tauri::command(async)]
pub fn start_team(
    app: AppHandle,
    state: State<'_, AppState>,
    team: String,
    task: Option<String>,
    pane_id: Option<String>,
) -> Result<String, String> {
    start_team_from(app, state, team, task, pane_id, None, "you".into())
}

/// [`start_team`], its task logged as given by `from`: a person, from the
/// app; whoever ran `mira team start`, from the socket. `title` is the
/// task's card's, instead of one made from the task.
pub fn start_team_from(
    app: AppHandle,
    state: State<'_, AppState>,
    team: String,
    task: Option<String>,
    pane_id: Option<String>,
    title: Option<String>,
    from: String,
) -> Result<String, String> {
    let cwd = cwd_of(&state, pane_id.as_deref());
    let catalog = cmux_core::roles::catalog(cwd.as_deref().map(std::path::Path::new));
    let def = catalog
        .teams
        .iter()
        .find(|t| t.name == team)
        .ok_or_else(|| format!("no team `{team}` (`mira team list --defined` lists them)"))?;
    if let Some(problem) = &def.problem {
        return Err(format!("team `{team}`: {problem}"));
    }
    let seats = cmux_core::teams::seats(def);
    let mira = mira_for_prompt();
    let mut launches = Vec::new();
    for seat in &seats {
        let roster = cmux_core::teams::seat_layers(&catalog, def, seat, &seats, &mira);
        let task = task.as_deref().filter(|_| seat.lead);
        let launch = prepare_launch(Some(&seat.role), cmux_protocol::Harness::Claude, Some(&catalog), cwd.as_deref(), &roster, Some(&seat.name), task)
            .map_err(|e| format!("team `{team}`, member `{}`: {e}", seat.name))?;
        launches.push(launch);
    }

    let panes = cmux_core::state::Workspace::fresh_pane_ids(seats.len());
    let run = TeamRun {
        name: def.name.clone(),
        members: seats
            .iter()
            .zip(&panes)
            .map(|(s, p)| TeamSeat { name: s.name.clone(), role: s.role.clone(), pane: p.clone(), lead: s.lead })
            .collect(),
    };
    let root = cmux_core::teams::layout(&panes, def.layout.as_deref());
    let tab = state.workspace.lock().unwrap().new_team_tab(root, run, &def.name);
    {
        let mut meta = state.meta.lock().unwrap();
        for (pane, launch) in panes.iter().zip(launches) {
            mark_launched(meta.entry(pane.clone()).or_default(), launch, cwd.clone());
        }
    }
    emit_workspace(&app);
    let lead = seats.iter().zip(&panes).find(|(s, _)| s.lead).map(|(_, p)| p);
    if let (Some(lead), Some(task)) = (lead, &task) {
        crate::tasks::given(&app, cmux_protocol::TaskSource::TeamStart, from, lead, task, None, title);
    }
    Ok(tab)
}

/// What team `team` — the one `pane_id`'s directory sees — tells its
/// members, in tokens, and the least that costs per turn
/// (`teams::team_weight`).
#[tauri::command]
pub fn team_cost(state: State<'_, AppState>, team: String, pane_id: Option<String>) -> Result<cmux_protocol::TeamWeight, String> {
    let cwd = cwd_of(&state, pane_id.as_deref());
    let catalog = cmux_core::roles::catalog(cwd.as_deref().map(std::path::Path::new));
    let def = catalog
        .teams
        .iter()
        .find(|t| t.name == team)
        .ok_or_else(|| format!("no team `{team}` (`mira team list --defined` lists them)"))?;
    let warn_usd = state.config.lock().unwrap().team_cost_warn_usd;
    cmux_core::teams::team_weight(&catalog, def, &mira_for_prompt(), warn_usd)
}

/// Every running team, for the frontend.
#[tauri::command]
pub fn list_teams(state: State<'_, AppState>) -> Vec<TeamInfo> {
    team_list(&state)
}

/// Every running team, with where each member is.
pub fn team_list(state: &AppState) -> Vec<TeamInfo> {
    let ws = state.workspace.lock().unwrap();
    let meta = state.meta.lock().unwrap();
    ws.tabs
        .iter()
        .filter_map(|tab| {
            let run = tab.team.as_ref()?;
            let in_tab = cmux_core::layout::pane_ids(&tab.root);
            Some(TeamInfo {
                tab_id: tab.id.clone(),
                name: run.name.clone(),
                members: run
                    .members
                    .iter()
                    .map(|seat| {
                        let m = meta.get(&seat.pane);
                        let state = cmux_core::teams::seat_state(
                            in_tab.contains(&seat.pane),
                            m.is_some_and(|m| m.is_agent()),
                        );
                        TeamMemberInfo {
                            name: seat.name.clone(),
                            role: seat.role.clone(),
                            pane: seat.pane.clone(),
                            lead: seat.lead,
                            status: m.filter(|_| state == SeatState::Running).and_then(|m| m.status()),
                            state,
                        }
                    })
                    .collect(),
            })
        })
        .collect()
}

/// The team of `pane_id`'s tab (the focused pane's when absent): its tab
/// id and what is running there.
pub(crate) fn team_here(state: &AppState, pane_id: Option<&str>) -> Result<(String, TeamRun), String> {
    let ws = state.workspace.lock().unwrap();
    let pane = pane_id.map(str::to_string).unwrap_or_else(|| ws.focused_pane());
    let tab = ws.tab_of_pane(&pane).ok_or_else(|| format!("no pane {pane}"))?;
    let run = tab
        .team
        .clone()
        .ok_or("this tab is not a team's: run it from a pane of the team's tab")?;
    Ok((tab.id.clone(), run))
}

/// What a member is told of its team now: the team as it stands, `with`
/// the member being started.
pub(crate) fn roster_now(
    run: &TeamRun,
    catalog: &cmux_protocol::RoleCatalog,
    seat: &cmux_core::teams::Seat,
    extra: Option<&cmux_core::teams::Seat>,
) -> Vec<cmux_protocol::PromptLayer> {
    let mut seats: Vec<cmux_core::teams::Seat> = run
        .members
        .iter()
        .map(|s| cmux_core::teams::Seat { name: s.name.clone(), role: s.role.clone(), lead: s.lead })
        .collect();
    if let Some(extra) = extra {
        seats.push(extra.clone());
    }
    // The team as defined, for its charter and way — or, if its file has
    // gone since it started, just its name.
    let def = catalog.teams.iter().find(|t| t.name == run.name).cloned().unwrap_or_else(|| {
        cmux_protocol::AgentTeam { name: run.name.clone(), ..Default::default() }
    });
    cmux_core::teams::seat_layers(catalog, &def, seat, &seats, &mira_for_prompt())
}

/// Adds a `role` member to the team of `pane_id`'s tab, split from that
/// pane. Its teammates learn of it from `team list`. Returns its pane.
#[tauri::command(async)]
pub fn team_add(
    app: AppHandle,
    state: State<'_, AppState>,
    role: String,
    name: Option<String>,
    pane_id: Option<String>,
) -> Result<String, String> {
    let (tab, run) = team_here(&state, pane_id.as_deref())?;
    if let Some(n) = name.as_deref().filter(|n| !cmux_core::roles::is_name(n)) {
        return Err(format!("`{n}` cannot name a member"));
    }
    let taken: Vec<&str> = run.members.iter().map(|s| s.name.as_str()).collect();
    let seat = cmux_core::teams::Seat {
        name: cmux_core::teams::new_seat_name(name.as_deref().unwrap_or(&role), &taken),
        role: role.clone(),
        lead: false,
    };
    let source = pane_id.unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    let cwd = cwd_of(&state, Some(&source));
    let catalog = cmux_core::roles::catalog(cwd.as_deref().map(std::path::Path::new));
    let roster = roster_now(&run, &catalog, &seat, Some(&seat));
    let launch = prepare_launch(Some(&role), cmux_protocol::Harness::Claude, Some(&catalog), cwd.as_deref(), &roster, Some(&seat.name), None)?;

    let pane = {
        let mut ws = state.workspace.lock().unwrap();
        let pane = ws.split_pane(&source, SplitDir::Row).ok_or_else(|| format!("no pane {source}"))?;
        if let Some(t) = ws.tab_mut(&tab).and_then(|t| t.team.as_mut()) {
            t.members.push(TeamSeat { name: seat.name.clone(), role, pane: pane.clone(), lead: false });
        }
        pane
    };
    mark_launched(state.meta.lock().unwrap().entry(pane.clone()).or_default(), launch, cwd);
    emit_workspace(&app);
    Ok(pane)
}

/// What `team restart` or `team clear` did for each member it was asked
/// about.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Restarted {
    pub name: String,
    pub pane: String,
    /// Restarted: "restarted", "reopened", or why not: "running", "busy".
    /// Cleared: see `team_clear`.
    pub outcome: String,
}

/// Starts again the members of `pane_id`'s team (`members` by name, all
/// when empty) whose agent exited — typed into its shell, when that shell
/// is idle — or whose pane was closed — in a new pane beside a teammate.
/// A running member is left alone.
#[tauri::command(async)]
pub fn team_restart(
    app: AppHandle,
    state: State<'_, AppState>,
    members: Vec<String>,
    pane_id: Option<String>,
) -> Result<Vec<Restarted>, String> {
    let (tab, run) = team_here(&state, pane_id.as_deref())?;
    if let Some(unknown) = members.iter().find(|m| !run.members.iter().any(|s| &&s.name == m)) {
        return Err(format!("no member `{unknown}` in team `{}`", run.name));
    }
    let wanted: Vec<&TeamSeat> = run
        .members
        .iter()
        .filter(|s| members.is_empty() || members.contains(&s.name))
        .collect();
    let cwd = cwd_of(&state, pane_id.as_deref());
    let catalog = cmux_core::roles::catalog(cwd.as_deref().map(std::path::Path::new));
    #[cfg(unix)]
    let procs = cmux_core::agents::process_table();
    let shell_pids: std::collections::HashMap<String, u32> = state.pty.pids().into_iter().collect();

    let mut out = Vec::new();
    for seat in wanted {
        let (in_tab, is_agent) = {
            let ws = state.workspace.lock().unwrap();
            let in_tab = ws
                .tabs
                .iter()
                .find(|t| t.id == tab)
                .is_some_and(|t| cmux_core::layout::contains(&t.root, &seat.pane));
            (in_tab, state.meta.lock().unwrap().get(&seat.pane).is_some_and(|m| m.is_agent()))
        };
        let seat_state = cmux_core::teams::seat_state(in_tab, is_agent);
        let report = |pane: &str, outcome: &str| Restarted {
            name: seat.name.clone(),
            pane: pane.to_string(),
            outcome: outcome.to_string(),
        };
        if seat_state == SeatState::Running {
            out.push(report(&seat.pane, "running"));
            continue;
        }
        // Typing into a shell that is running something types into that.
        #[cfg(unix)]
        if seat_state == SeatState::Exited {
            if let Some(&pid) = shell_pids.get(&seat.pane) {
                if cmux_core::agents::shell_busy(&procs, pid) {
                    out.push(report(&seat.pane, "busy"));
                    continue;
                }
            }
        }
        let me = cmux_core::teams::Seat { name: seat.name.clone(), role: seat.role.clone(), lead: seat.lead };
        let roster = roster_now(&run, &catalog, &me, None);
        let launch = prepare_launch(Some(&seat.role), cmux_protocol::Harness::Claude, Some(&catalog), cwd.as_deref(), &roster, Some(&seat.name), None)
            .map_err(|e| format!("member `{}`: {e}", seat.name))?;

        match seat_state {
            SeatState::Exited => {
                let input = launch.input.clone();
                mark_launched(state.meta.lock().unwrap().entry(seat.pane.clone()).or_default(), launch, cwd.clone());
                // A shell already printing takes it now; one not yet spawned
                // (a tab never shown since the restart) types it on its
                // first output, like a restored agent.
                if shell_pids.contains_key(&seat.pane) {
                    if let Some(m) = state.meta.lock().unwrap().get_mut(&seat.pane) {
                        m.startup_input = None;
                    }
                    state.pty.write(&seat.pane, format!("{input}\r").as_bytes())?;
                }
                out.push(report(&seat.pane, "restarted"));
            }
            SeatState::Closed => {
                // Beside a teammate still there, else wherever the tab's
                // focus is; the seat follows its new pane.
                let pane = {
                    let mut ws = state.workspace.lock().unwrap();
                    let Some(t) = ws.tabs.iter().find(|t| t.id == tab) else {
                        return Err("the team's tab has closed".into());
                    };
                    let beside = run
                        .members
                        .iter()
                        .map(|s| s.pane.clone())
                        .find(|p| cmux_core::layout::contains(&t.root, p))
                        .unwrap_or_else(|| t.focused.clone());
                    let pane = ws.split_pane(&beside, SplitDir::Row).ok_or("could not split the team's tab")?;
                    if let Some(s) = ws
                        .tab_mut(&tab)
                        .and_then(|t| t.team.as_mut())
                        .and_then(|r| r.members.iter_mut().find(|s| s.name == seat.name))
                    {
                        s.pane = pane.clone();
                    }
                    pane
                };
                mark_launched(state.meta.lock().unwrap().entry(pane.clone()).or_default(), launch, cwd.clone());
                out.push(report(&pane, "reopened"));
            }
            SeatState::Running => unreachable!(),
        }
    }
    emit_workspace(&app);
    Ok(out)
}

/// Starts every running member of `pane_id`'s team on a new conversation,
/// as `agent clear` does one: its teammates first and the lead last, so a
/// lead that runs this clears itself only once the others are done.
/// `outcome` is "cleared", or why not: "not running", "needs you" (asking
/// a person, or showing a menu), "no harness".
pub fn team_clear(app: &tauri::AppHandle, pane_id: Option<&str>, force: bool) -> Result<Vec<Restarted>, String> {
    use tauri::Manager;
    let state = &*app.state::<AppState>();
    let (tab, run) = team_here(state, pane_id)?;
    let mut seats: Vec<&TeamSeat> = run.members.iter().collect();
    seats.sort_by_key(|s| s.lead);
    Ok(seats
        .into_iter()
        .map(|seat| {
            let in_tab = {
                let ws = state.workspace.lock().unwrap();
                ws.tabs.iter().find(|t| t.id == tab).is_some_and(|t| cmux_core::layout::contains(&t.root, &seat.pane))
            };
            let outcome = if !in_tab {
                "not running"
            } else {
                crate::handoff::clear_pane(app, &seat.pane, force).map_or_else(|(outcome, _)| outcome, |()| "cleared")
            };
            Restarted { name: seat.name.clone(), pane: seat.pane.clone(), outcome: outcome.to_string() }
        })
        .collect())
}
