//! The agent editor pane: the role, way and team files a directory sees,
//! read, checked, saved and deleted through `cmux_core::role_files`, which
//! decides what the page may touch. The page only ever names a path it was
//! given in a catalog; every one is checked again here.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, State};

use cmux_core::role_files::{Kind, Places};
use cmux_protocol::{CatalogKind, FileFields, HarnessChoices, PromptLayer, RoleCatalog, RoleSource, SplitDir, UsedBy};

use crate::commands::emit_workspace;
use crate::AppState;

/// Opens an editor pane for the files `pane_id`'s directory sees (the
/// focused pane's when absent): a split of it, or a tab of its own.
#[tauri::command]
pub fn open_agent_editor(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: Option<String>,
    tab: bool,
) -> Result<String, String> {
    let source = pane_id
        .filter(|id| state.meta.lock().unwrap().contains_key(id))
        .unwrap_or_else(|| state.workspace.lock().unwrap().focused_pane());
    let (cwd, repo_root) = {
        let meta = state.meta.lock().unwrap();
        let m = meta.get(&source);
        (
            m.and_then(|m| m.cwd.clone()).or_else(cmux_core::config::default_cwd),
            m.and_then(|m| m.repo_root.clone()),
        )
    };
    // In a tab of its own, an editor already open on the same files — the
    // same repository, or outside one the same directory — is shown again
    // rather than opened twice. A split always opens one.
    if tab {
        let mut ws = state.workspace.lock().unwrap();
        let existing = {
            let meta = state.meta.lock().unwrap();
            ws.find_pane(|p| {
                meta.get(p).is_some_and(|m| {
                    m.agent_editor
                        && match (&repo_root, &m.repo_root) {
                            (Some(a), Some(b)) => a == b,
                            _ => m.cwd == cwd,
                        }
                })
            })
        };
        if let Some(pane) = existing {
            ws.focus_pane(&pane);
            drop(ws);
            emit_workspace(&app);
            return Ok(pane);
        }
    }
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
    {
        let mut meta = state.meta.lock().unwrap();
        let entry = meta.entry(pane.clone()).or_default();
        entry.cwd = cwd;
        entry.repo_root = repo_root;
        entry.agent_editor = true;
    }
    emit_workspace(&app);
    Ok(pane)
}

/// Where an editor pane's files are: its repository's `.mirador`, and
/// the user's config directory.
fn places(state: &AppState, pane_id: &str) -> Result<(Places, Option<String>), String> {
    let meta = state.meta.lock().unwrap();
    let m = meta.get(pane_id).filter(|m| m.agent_editor).ok_or("not an agent editor pane")?;
    let places = Places::of(m.cwd.as_deref().map(Path::new), m.repo_root.as_deref().map(Path::new));
    Ok((places, m.cwd.clone()))
}

/// What the editor shows: the catalog its directory sees, and where a new
/// file would go in each place (none for the repository outside one).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorCatalog {
    pub catalog: RoleCatalog,
    pub repo_dir: Option<String>,
    pub global_dir: String,
    /// What the role form's pickers offer.
    pub choices: Vec<HarnessChoices>,
}

fn catalog(state: &AppState, pane_id: &str) -> Result<EditorCatalog, String> {
    let (places, cwd) = places(state, pane_id)?;
    Ok(EditorCatalog {
        catalog: cmux_core::roles::catalog(cwd.as_deref().map(Path::new)),
        repo_dir: places.repo.map(|p| p.to_string_lossy().into_owned()),
        global_dir: places.global.to_string_lossy().into_owned(),
        choices: cmux_core::roles::role_choices(),
    })
}

#[tauri::command]
pub fn agent_editor_catalog(state: State<'_, AppState>, pane_id: String) -> Result<EditorCatalog, String> {
    catalog(&state, &pane_id)
}

/// The teams and roles that use the role or way `name`, from what the
/// editor's directory sees, each running or not: a team of that name
/// running here, an agent started with that role.
#[tauri::command]
pub fn agent_editor_used_by(
    state: State<'_, AppState>,
    pane_id: String,
    kind: CatalogKind,
    name: String,
) -> Result<Vec<UsedBy>, String> {
    let catalog = catalog(&state, &pane_id)?.catalog;
    let runs = crate::teams::team_list(&state);
    let running = |m: &cmux_protocol::TeamMemberInfo| m.state == cmux_protocol::SeatState::Running;
    let teams: Vec<String> = runs.iter().filter(|t| t.members.iter().any(running)).map(|t| t.name.clone()).collect();
    // A standalone agent's pane names its role; a team member's names its
    // seat (`reviewer-2`), so its role comes from the team it runs in.
    let mut roles: Vec<String> = state
        .meta
        .lock()
        .unwrap()
        .values()
        .filter(|m| m.is_agent())
        .filter_map(|m| m.agent_role.clone())
        .collect();
    roles.extend(runs.iter().flat_map(|t| t.members.iter().filter(|m| running(m)).map(|m| m.role.clone())));
    let teams: Vec<&str> = teams.iter().map(String::as_str).collect();
    let roles: Vec<&str> = roles.iter().map(String::as_str).collect();
    Ok(cmux_core::roles::used_by(&catalog, kind, &name, &teams, &roles))
}

/// What a member is told: the layers, and the prompt they join into.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Told {
    pub layers: Vec<PromptLayer>,
    pub prompt: Option<String>,
}

/// What `member` of `team` is told when the team starts, as the layers it
/// is made of — the very ones the launch joins — and joined as it joins
/// them.
#[tauri::command]
pub fn agent_editor_told(
    state: State<'_, AppState>,
    pane_id: String,
    team: String,
    member: String,
) -> Result<Told, String> {
    let catalog = catalog(&state, &pane_id)?.catalog;
    let def = catalog.teams.iter().find(|t| t.name == team).ok_or_else(|| format!("no team `{team}`"))?;
    let seats = cmux_core::teams::seats(def);
    let seat = seats.iter().find(|s| s.name == member).ok_or_else(|| format!("no member `{member}` in team `{team}`"))?;
    let (_, mut layers) = cmux_core::roles::role_layers(&catalog, &seat.role)?;
    layers.extend(cmux_core::teams::seat_layers(&catalog, def, seat, &seats, &crate::teams::mira_for_prompt()));
    let prompt = cmux_core::roles::join_layers(&layers);
    Ok(Told { layers, prompt })
}

#[tauri::command]
pub fn agent_file_read(state: State<'_, AppState>, pane_id: String, path: String) -> Result<String, String> {
    places(&state, &pane_id)?.0.read(&PathBuf::from(path))
}

/// Warnings for `text` as a `kind`, or why it cannot be saved.
#[tauri::command]
pub fn agent_file_check(kind: String, name: String, text: String) -> Result<Vec<String>, String> {
    cmux_core::role_files::check(Kind::parse(&kind)?, &name, &text)
}

#[tauri::command]
pub fn agent_file_template(kind: String) -> Result<String, String> {
    Ok(cmux_core::role_files::template(Kind::parse(&kind)?).to_string())
}

/// Saves a file — `create` a new one — and gives the catalog as it is now.
#[tauri::command]
pub fn agent_file_save(
    state: State<'_, AppState>,
    pane_id: String,
    kind: String,
    source: RoleSource,
    name: String,
    text: String,
    create: bool,
) -> Result<EditorCatalog, String> {
    places(&state, &pane_id)?.0.save(Kind::parse(&kind)?, source, &name, &text, create)?;
    catalog(&state, &pane_id)
}

/// Saves a form's `fields` into the team, role or way `name` in `source`'s
/// place: applied to the text of `from` — the file as it is, or a
/// built-in's `builtin:` name, which is saved as a new file of yours or
/// the repository's (`create`) — or to nothing for a new one. Checked and
/// written as `agent_file_save` writes text.
#[tauri::command]
pub fn agent_file_save_fields(
    state: State<'_, AppState>,
    pane_id: String,
    source: RoleSource,
    name: String,
    from: Option<String>,
    fields: FileFields,
    create: bool,
) -> Result<EditorCatalog, String> {
    let places = places(&state, &pane_id)?.0;
    let text = match &from {
        Some(path) => places.read(&PathBuf::from(path))?,
        None => String::new(),
    };
    let (kind, text) = match &fields {
        FileFields::Team(f) => (Kind::Team, cmux_core::teams::set_team_fields(&text, f)?),
        FileFields::Role(f) => (Kind::Role, cmux_core::roles::set_role_fields(&text, f)?),
        FileFields::Way(f) => (Kind::Way, cmux_core::roles::set_way_fields(&text, f)?),
    };
    places.save(kind, source, &name, &text, create)?;
    catalog(&state, &pane_id)
}

#[tauri::command]
pub fn agent_file_delete(state: State<'_, AppState>, pane_id: String, path: String) -> Result<EditorCatalog, String> {
    places(&state, &pane_id)?.0.delete(&PathBuf::from(path))?;
    catalog(&state, &pane_id)
}
