//! Roles and ways of working, from Markdown files.
//!
//! A role is a named way to start an agent; a way of working is a playbook
//! any role can follow. Both are Markdown with YAML frontmatter, named by
//! their file:
//!
//! ```text
//! <repo>/.mirador/roles/reviewer.md     the repository's, shared through git
//! <config>/roles/reviewer.md            the user's, everywhere
//! <repo>/.mirador/ways/our-way.md
//! <config>/ways/our-way.md
//! ```
//!
//! A role's frontmatter says how it starts (`harness`, `model`, `effort`,
//! `profile`, `way`, `description`) and its body is its prompt; a way's body
//! is its prompt.
//! The same name in two places means the repository's: a team that commits
//! a `reviewer` gets that reviewer, whatever its members have of their own.
//! `agentRoles` entries in mirador.json are roles too, below the files.
//!
//! Nothing here is dropped silently. A file that does not parse, a field
//! that is not one, a harness a role cannot start yet, a way nobody wrote:
//! each is a problem the CLI and the palette can show, because a role
//! that quietly lost its way of working is worse than one that will not
//! start.

use std::path::{Path, PathBuf};

use cmux_protocol::{
    AgentRole, AgentTeam, AgentWay, CatalogKind, Harness, HarnessChoices, PromptLayer, PromptLayerKind, RoleCatalog,
    RoleFields, RoleSource, ShadowedFile, ToolRules, UsedBy,
};
use serde::Deserialize;

/// A prompt longer than this is not a prompt: refused, not read.
const MAX_FILE: u64 = 256 * 1024;

/// The repository's `.mirador` directory: the nearest one at or above
/// `cwd`.
pub fn repo_dir(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .map(|dir| dir.join(".mirador"))
        .find(|dir| dir.is_dir())
}

/// Everything `cwd` sees, read from disk now: the repository's files, the
/// user's, and mirador.json's `agentRoles`, merged by name.
pub fn catalog(cwd: Option<&Path>) -> RoleCatalog {
    let repo = cwd.and_then(repo_dir);
    catalog_in(repo.as_deref(), &crate::config::config_dir(), &crate::config::agent_roles())
}

/// [`catalog`], with the directories given: `repo` is a `.mirador`
/// directory, `global` the config directory.
pub fn catalog_in(repo: Option<&Path>, global: &Path, config_roles: &[AgentRole]) -> RoleCatalog {
    let mut problems = Vec::new();

    let mut roles: Vec<AgentRole> = Vec::new();
    let mut ways: Vec<AgentWay> = Vec::new();
    let mut teams: Vec<AgentTeam> = Vec::new();
    // Each one a later definition replaced: kind, name, source, path.
    let mut hidden: Vec<(CatalogKind, String, RoleSource, Option<String>)> = Vec::new();
    // What ships with Mirador first, so that anything of the same name —
    // an `agentRoles` entry, your file, the repository's — replaces it.
    for (path, name, text) in crate::builtin::of("ways") {
        let (description, prompt) = parse_way(text).expect("a built-in way parses (tested)");
        ways.push(AgentWay { name, prompt, description, source: RoleSource::Builtin, path });
    }
    for (path, name, text) in crate::builtin::of("roles") {
        let (role, _) = parse_role(&name, text).expect("a built-in role parses (tested)");
        roles.push(AgentRole { source: RoleSource::Builtin, path: Some(path), ..role });
    }
    for (path, name, text) in crate::builtin::of("teams") {
        let (team, _) = crate::teams::parse_team(&name, std::path::Path::new(&path), RoleSource::Builtin, text)
            .expect("a built-in team parses (tested)");
        teams.push(team);
    }
    for role in config_roles {
        if let Some(r) = replace(&mut roles, AgentRole { source: RoleSource::Config, path: None, ..role.clone() }, |r| &r.name) {
            hidden.push((CatalogKind::Role, r.name, r.source, r.path));
        }
    }
    let dirs = [(Some(global), RoleSource::Global), (repo, RoleSource::Repo)];
    for (dir, source) in dirs {
        let Some(dir) = dir else { continue };
        for (path, name, text) in read_dir(&dir.join("ways"), &mut problems) {
            match parse_way(&text) {
                Ok((description, prompt)) => {
                    let way = AgentWay { name, prompt, description, source, path: display(&path) };
                    if let Some(w) = replace(&mut ways, way, |w| &w.name) {
                        hidden.push((CatalogKind::Way, w.name, w.source, Some(w.path)));
                    }
                }
                Err(e) => problems.push(format!("{}: {e}", display(&path))),
            }
        }
        for (path, name, text) in read_dir(&dir.join("teams"), &mut problems) {
            match crate::teams::parse_team(&name, &path, source, &text) {
                Ok((team, ignored)) => {
                    for field in ignored {
                        problems.push(format!("{}: `{field}` is not a team field; ignored", display(&path)));
                    }
                    if let Some(t) = replace(&mut teams, team, |t| &t.name) {
                        hidden.push((CatalogKind::Team, t.name, t.source, Some(t.path)));
                    }
                }
                Err(e) => problems.push(format!("{}: {e}", display(&path))),
            }
        }
        for (path, name, text) in read_dir(&dir.join("roles"), &mut problems) {
            match parse_role(&name, &text) {
                Ok((role, ignored)) => {
                    for field in ignored {
                        problems.push(format!("{}: `{field}` is not a role field; ignored", display(&path)));
                    }
                    let role = AgentRole { source, path: Some(display(&path)), ..role };
                    if let Some(r) = replace(&mut roles, role, |r| &r.name) {
                        hidden.push((CatalogKind::Role, r.name, r.source, r.path));
                    }
                }
                Err(e) => problems.push(format!("{}: {e}", display(&path))),
            }
        }
    }

    // A role that cannot start stays listed — it is still the role you
    // wrote — but says why, here and on the role.
    for role in &mut roles {
        role.problem = problem_of(role, &ways);
        if let Some(problem) = &role.problem {
            problems.push(format!("{}: {problem}", role.path.as_deref().unwrap_or("agentRoles")));
        }
    }
    // A team is only as startable as its members' roles, so after them.
    for team in &mut teams {
        team.problem = crate::teams::problem_of(team, &roles, &ways);
        if let Some(problem) = &team.problem {
            problems.push(format!("{}: {problem}", team.path));
        }
    }
    roles.sort_by(|a, b| a.name.cmp(&b.name));
    ways.sort_by(|a, b| a.name.cmp(&b.name));
    teams.sort_by(|a, b| a.name.cmp(&b.name));
    // What is used instead of each hidden one: whichever came last.
    let mut shadowed: Vec<ShadowedFile> = hidden
        .into_iter()
        .filter_map(|(kind, name, source, path)| {
            let (by, by_path) = match kind {
                CatalogKind::Role => roles.iter().find(|r| r.name == name).map(|r| (r.source, r.path.clone()))?,
                CatalogKind::Way => ways.iter().find(|w| w.name == name).map(|w| (w.source, Some(w.path.clone())))?,
                CatalogKind::Team => teams.iter().find(|t| t.name == name).map(|t| (t.source, Some(t.path.clone())))?,
            };
            Some(ShadowedFile { kind, name, source, path, by, by_path })
        })
        .collect();
    shadowed.sort_by(|a, b| a.name.cmp(&b.name));
    RoleCatalog { roles, ways, teams, problems, shadowed }
}

/// Who uses the role or way of working `name` (`kind`): for a role, the
/// teams with it as a member; for a way, the teams that follow it as
/// theirs, then the roles that do. Each says whether it runs, from the
/// names of the teams running and of the roles agents run with.
pub fn used_by(catalog: &RoleCatalog, kind: CatalogKind, name: &str, running_teams: &[&str], running_roles: &[&str]) -> Vec<UsedBy> {
    let team = |t: &AgentTeam| UsedBy { kind: CatalogKind::Team, name: t.name.clone(), running: running_teams.contains(&t.name.as_str()) };
    match kind {
        CatalogKind::Role => catalog.teams.iter().filter(|t| t.members.iter().any(|m| m.role == name)).map(team).collect(),
        CatalogKind::Way => catalog
            .teams
            .iter()
            .filter(|t| t.way.as_deref() == Some(name))
            .map(team)
            .chain(catalog.roles.iter().filter(|r| r.way.as_deref() == Some(name)).map(|r| UsedBy {
                kind: CatalogKind::Role,
                name: r.name.clone(),
                running: running_roles.contains(&r.name.as_str()),
            }))
            .collect(),
        CatalogKind::Team => Vec::new(),
    }
}

/// The harnesses a role knows how to start: the ones whose command line
/// takes a model and a prompt Mirador knows how to give.
pub fn can_start(harness: Harness) -> bool {
    matches!(harness, Harness::Claude | Harness::Codex)
}

/// A role ready to start: the role, and its whole prompt — its own, then
/// its way of working's.
pub fn resolve(catalog: &RoleCatalog, name: &str) -> Result<(AgentRole, Option<String>), String> {
    let (role, layers) = role_layers(catalog, name)?;
    Ok((role, join_layers(&layers)))
}

/// A role ready to start, and its prompt as the layers it is made of: its
/// own, then its way of working's, each with the file it comes from.
pub fn role_layers(catalog: &RoleCatalog, name: &str) -> Result<(AgentRole, Vec<PromptLayer>), String> {
    let role = catalog
        .roles
        .iter()
        .find(|r| r.name == name)
        .ok_or_else(|| format!("no role `{name}` (`mira agent roles` lists them)"))?;
    if let Some(problem) = &role.problem {
        return Err(format!("role `{name}`: {problem}"));
    }
    let mut layers = Vec::new();
    if let Some(prompt) = &role.prompt {
        let source = role.path.clone().unwrap_or_else(|| "agentRoles in mirador.json".into());
        layers.push(PromptLayer { kind: PromptLayerKind::Role, source, text: prompt.clone() });
    }
    if let Some(way) = role.way.as_ref().and_then(|way| catalog.ways.iter().find(|w| &w.name == way)) {
        layers.push(PromptLayer { kind: PromptLayerKind::RoleWay, source: way.path.clone(), text: way.prompt.clone() });
    }
    Ok((role.clone(), layers))
}

/// The prompt an agent is given, from its layers: the role's and its
/// way's each trimmed, then a team member's own layers as one — they are
/// written to follow each other — with a blank line between, and nothing
/// for what is empty.
pub fn join_layers(layers: &[PromptLayer]) -> Option<String> {
    let is_role = |l: &&PromptLayer| matches!(l.kind, PromptLayerKind::Role | PromptLayerKind::RoleWay);
    let team: String = layers.iter().filter(|l| !is_role(l)).map(|l| l.text.as_str()).collect();
    let prompt = layers
        .iter()
        .filter(is_role)
        .map(|l| l.text.as_str())
        .chain([team.as_str()])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    (!prompt.is_empty()).then_some(prompt)
}

/// Puts `item` in `list`, in place of one with the same name, which it
/// gives back.
fn replace<T>(list: &mut Vec<T>, item: T, name: impl Fn(&T) -> &String) -> Option<T> {
    match list.iter().position(|x| name(x) == name(&item)) {
        Some(i) => Some(std::mem::replace(&mut list[i], item)),
        None => {
            list.push(item);
            None
        }
    }
}

/// The `.md` files of `dir` that can be read: path, name, text. A missing
/// directory has none, and is not a problem.
pub(crate) fn read_dir(dir: &Path, problems: &mut Vec<String>) -> Vec<(PathBuf, String, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let name = path.file_stem()?.to_string_lossy().into_owned();
            if !is_name(&name) {
                problems.push(format!(
                    "{}: a name is letters, digits, `-`, `_` and `.`, not starting with `-` or `.`",
                    display(&path)
                ));
                return None;
            }
            let meta = std::fs::metadata(&path).ok()?;
            if !meta.is_file() {
                return None;
            }
            if meta.len() > MAX_FILE {
                problems.push(format!("{}: larger than {} KB; not read", display(&path), MAX_FILE / 1024));
                return None;
            }
            match std::fs::read_to_string(&path) {
                Ok(text) => Some((path, name, text)),
                Err(e) => {
                    problems.push(format!("{}: {e}", display(&path)));
                    None
                }
            }
        })
        .collect()
}

/// Whether `name` can name a role: it goes on command lines (`--name`,
/// `mira agent new --role`) and in palette entries, so nothing a shell or
/// a flag parser would read twice.
pub fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(['-', '.'])
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RoleFront {
    harness: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    profile: Option<String>,
    way: Option<String>,
    description: Option<String>,
    mcp: Option<Vec<String>>,
    plugins: Option<Vec<String>>,
    skills: Option<Vec<String>>,
    tools: Option<ToolRules>,
    #[serde(flatten)]
    other: std::collections::BTreeMap<String, serde_yaml_ng::Value>,
}

/// A role file: the role, and the frontmatter keys it did not know.
pub(crate) fn parse_role(name: &str, text: &str) -> Result<(AgentRole, Vec<String>), String> {
    let (front, body) = split_frontmatter(text)?;
    let front: RoleFront = match front {
        Some(yaml) if !yaml.trim().is_empty() => {
            serde_yaml_ng::from_str(yaml).map_err(|e| format!("frontmatter: {e}"))?
        }
        _ => RoleFront::default(),
    };
    let harness = match present(front.harness) {
        Some(h) => crate::agents::harness_named(&h).ok_or_else(|| format!("unknown harness `{h}`"))?,
        None => Harness::Claude,
    };
    let way = present(front.way);
    if let Some(way) = &way {
        if !is_name(way) {
            return Err(format!("`{way}` cannot name a way of working"));
        }
    }
    // It names a file, `<name>.config.toml` beside Codex's config.toml.
    let profile = present(front.profile);
    if let Some(profile) = &profile {
        if !is_name(profile) {
            return Err(format!("`{profile}` cannot name a Codex profile"));
        }
    }
    let role = AgentRole {
        name: name.to_string(),
        harness,
        model: present(front.model),
        effort: present(front.effort),
        profile,
        prompt: present(Some(body.to_string())),
        way,
        description: present(front.description).map(|d| one_line(&d)),
        mcp: front.mcp.map(names),
        plugins: front.plugins.map(names),
        skills: front.skills.map(names),
        tools: front.tools.map(|t| ToolRules { allow: names(t.allow), deny: names(t.deny) }),
        ..Default::default()
    };
    Ok((role, front.other.into_keys().collect()))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct WayFront {
    description: Option<String>,
}

/// A way file: its description and its prompt. Its frontmatter may say
/// more than a description — later versions may read more — but an empty
/// way is a mistake.
pub(crate) fn parse_way(text: &str) -> Result<(Option<String>, String), String> {
    let (front, body) = split_frontmatter(text)?;
    let front: WayFront = match front {
        Some(yaml) if !yaml.trim().is_empty() => {
            serde_yaml_ng::from_str(yaml).map_err(|e| format!("frontmatter: {e}"))?
        }
        _ => WayFront::default(),
    };
    let prompt = body.trim();
    if prompt.is_empty() {
        return Err("a way of working needs text after its frontmatter".into());
    }
    Ok((present(front.description).map(|d| one_line(&d)), prompt.to_string()))
}

/// Splits `---`-fenced frontmatter from the body. A file that does not
/// start with a fence has none; one that opens a fence must close it.
pub(crate) fn split_frontmatter(text: &str) -> Result<(Option<&str>, &str), String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) else {
        return Ok((None, text));
    };
    let mut at = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Ok((Some(&rest[..at]), &rest[at + line.len()..]));
        }
        at += line.len();
    }
    Err("frontmatter opened with `---` is never closed".into())
}

/// A file's frontmatter as a mapping, in the file's order, and its body.
pub(crate) fn front_and_body(text: &str) -> Result<(serde_yaml_ng::Mapping, String), String> {
    let (front, body) = split_frontmatter(text)?;
    let map = match front {
        Some(yaml) if !yaml.trim().is_empty() => {
            serde_yaml_ng::from_str(yaml).map_err(|e| format!("frontmatter: {e}"))?
        }
        _ => serde_yaml_ng::Mapping::new(),
    };
    Ok((map, body.to_string()))
}

/// Sets `key` to `value` where the file has it, else after its other keys;
/// removes it for none, keeping the order of the rest.
pub(crate) fn set_key(map: &mut serde_yaml_ng::Mapping, key: &str, value: Option<serde_yaml_ng::Value>) {
    match value {
        Some(v) => match map.get_mut(key) {
            Some(slot) => *slot = v,
            None => {
                map.insert(key.into(), v);
            }
        },
        None => {
            map.shift_remove(key);
        }
    }
}

/// A string field: none when blank, so the file never says `model: ''`.
pub(crate) fn text_value(v: &Option<String>) -> Option<serde_yaml_ng::Value> {
    present(v.clone()).map(serde_yaml_ng::Value::String)
}

/// A file written back from its frontmatter and its body, the way people
/// write them: no fence for no frontmatter, the body after it.
pub(crate) fn write_file(map: &serde_yaml_ng::Mapping, body: Option<&str>) -> Result<String, String> {
    let body = body.map(str::trim).filter(|b| !b.is_empty());
    let mut out = String::new();
    if !map.is_empty() {
        let yaml = serde_yaml_ng::to_string(map).map_err(|e| e.to_string())?;
        out.push_str(&format!("---\n{yaml}---\n"));
    }
    if let Some(body) = body {
        out.push_str(body);
        out.push('\n');
    }
    Ok(out)
}

/// `text`, a role file, with the role form's `fields`: the frontmatter
/// keys the form edits set (or removed when empty or the default — Claude
/// Code is no `harness` key), the body its prompt. Every other key stays,
/// where it was, and so do the tools rules it does not edit (`allow`).
/// The frontmatter is rewritten, so comments in it are lost.
pub fn set_role_fields(text: &str, fields: &RoleFields) -> Result<String, String> {
    use serde_yaml_ng::Value;
    let (mut map, _) = front_and_body(text)?;
    let harness = (fields.harness != Harness::Claude).then(|| Value::String(crate::agents::program_of(fields.harness).into()));
    set_key(&mut map, "harness", harness);
    set_key(&mut map, "model", text_value(&fields.model));
    set_key(&mut map, "effort", text_value(&fields.effort));
    set_key(&mut map, "way", text_value(&fields.way));
    set_key(&mut map, "description", text_value(&fields.description));
    let mut tools = match map.get("tools") {
        Some(Value::Mapping(t)) => t.clone(),
        _ => serde_yaml_ng::Mapping::new(),
    };
    let deny: Vec<Value> = names(fields.deny.clone()).into_iter().map(Value::String).collect();
    set_key(&mut tools, "deny", (!deny.is_empty()).then_some(Value::Sequence(deny)));
    set_key(&mut map, "tools", (!tools.is_empty()).then_some(Value::Mapping(tools)));
    write_file(&map, fields.prompt.as_deref())
}

/// What a role form offers, per harness a role can start: the models the
/// repository's own roles and Claude Code's aliases name, and the efforts
/// a role is checked against.
pub fn role_choices() -> Vec<HarnessChoices> {
    let models: fn(Harness) -> &'static [&'static str] = |h| match h {
        // `--model` takes an alias or a full name.
        Harness::Claude => &["opus", "sonnet", "haiku", "claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"],
        // What the built-in reviewer and the live tests run.
        Harness::Codex => &["gpt-6-luna"],
        _ => &[],
    };
    [Harness::Claude, Harness::Codex]
        .into_iter()
        .filter(|h| can_start(*h))
        .map(|harness| HarnessChoices {
            harness,
            models: models(harness).iter().map(|m| m.to_string()).collect(),
            efforts: efforts(harness).iter().map(|e| e.to_string()).collect(),
        })
        .collect()
}

/// A list's entries, trimmed, without the empty ones.
fn names(list: Vec<String>) -> Vec<String> {
    list.into_iter().filter_map(|n| present(Some(n))).collect()
}

/// Why `role` cannot start, if it cannot.
fn problem_of(role: &AgentRole, ways: &[AgentWay]) -> Option<String> {
    if !can_start(role.harness) {
        return Some(format!("a role cannot start {} yet, only Claude Code and Codex", role.harness.label()));
    }
    if let Some(effort) = &role.effort {
        let efforts = efforts(role.harness);
        if !efforts.contains(&effort.as_str()) {
            return Some(format!(
                "effort `{effort}` is not one {} takes: {}",
                role.harness.label(),
                efforts.join(", ")
            ));
        }
    }
    if role.profile.is_some() && role.harness != Harness::Codex {
        return Some(format!("`profile` is Codex's: a {} role has none", role.harness.label()));
    }
    // A profile can turn the sandbox and the approvals off, and it is the
    // person's to choose, like `tools.allow`: a repository never picks one.
    if role.source == RoleSource::Repo && role.profile.is_some() {
        return Some("a repository's role cannot pick a Codex profile — put `profile` in a role of your own".into());
    }
    if let Some(way) = role.way.as_ref().filter(|way| !ways.iter().any(|w| &&w.name == way)) {
        return Some(format!("way of working `{way}` is not in any ways/ directory"));
    }
    let limits = role.mcp.is_some() || role.plugins.is_some() || role.skills.is_some() || role.tools.is_some();
    if role.harness != Harness::Claude && limits {
        return Some(format!(
            "a {} role cannot limit its MCP servers, plugins, skills or tools yet — only Claude Code's can",
            role.harness.label()
        ));
    }
    // A repository can take tools away, never hand them out: rules given
    // at launch are Claude Code's highest-priority settings, and would
    // skip the trust it asks before a project's own settings apply.
    if role.source == RoleSource::Repo && role.tools.as_ref().is_some_and(|t| !t.allow.is_empty()) {
        return Some(
            "a repository's role cannot allow tools, only deny them — put `tools.allow` in a role of your own".into(),
        );
    }
    None
}

/// The efforts a harness takes, as its CLI spells them: Claude Code's
/// `--effort` (its `--help`), and the ones Codex's models list as supported
/// (0.163's model catalog). Lower case only: Codex reads any other case as
/// a name no model supports.
fn efforts(harness: Harness) -> &'static [&'static str] {
    match harness {
        Harness::Codex => &["low", "medium", "high", "xhigh", "max", "ultra"],
        _ => &["low", "medium", "high", "xhigh", "max"],
    }
}

fn present(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Where launch files are written — a role's prompt, its settings — for a
/// harness that reads them from a file.
pub fn prompts_dir() -> PathBuf {
    crate::session::data_dir().join("prompts")
}

/// How long a launch file nobody launched with is kept.
pub const PROMPT_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 60 * 60);

/// Writes a launch prompt and returns where.
///
/// The file is named after the role and its content, and outlives the
/// launch: the command that names it stays in the shell's history, and
/// ↑ Enter after `/exit` is how people start an agent again — which a file
/// deleted after the launch would break. The same prompt is the same file,
/// rewritten (so not pruned) each launch; a role edited is a new one.
pub fn write_prompt(role: &str, prompt: &str) -> Result<PathBuf, String> {
    write_launch_file(&prompts_dir(), role, "md", prompt)
}

/// Writes a role's Claude Code settings ([`crate::role_limits`]), named
/// the same way. A resumed conversation does not keep them — they are the
/// process's, not the conversation's — so a restore passes the same file
/// again, and the session names it.
pub fn write_settings(role: &str, settings: &serde_json::Value) -> Result<PathBuf, String> {
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    write_launch_file(&prompts_dir(), role, "settings.json", &text)
}

fn write_launch_file(dir: &Path, role: &str, ext: &str, content: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let role = if is_name(role) { role } else { "role" };
    let path = dir.join(format!("{role}-{:016x}.{ext}", fnv1a(content.as_bytes())));
    // Through a file of its own and a rename: a launch reading the same
    // file never sees it half written.
    let tmp = dir.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, content)
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("{}: {e}", path.display())
        })?;
    Ok(path)
}

/// Removes launch files no launch has written for [`PROMPT_MAX_AGE`],
/// except those in `keep` — the ones a restored pane will pass again: run
/// at startup.
pub fn prune_prompts(keep: &[PathBuf]) {
    prune_prompts_in(&prompts_dir(), PROMPT_MAX_AGE, keep);
}

fn prune_prompts_in(dir: &Path, max_age: std::time::Duration, keep: &[PathBuf]) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if keep.contains(&entry.path()) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > max_age);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// FNV-1a: a name for a prompt's content that is the same in every build,
/// which `DefaultHasher` does not promise.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("mira-roles-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn splits_frontmatter_from_the_body() {
        assert_eq!(split_frontmatter("---\nmodel: opus\n---\nYou review.\n"), Ok((Some("model: opus\n"), "You review.\n")));
        assert_eq!(split_frontmatter("\u{feff}---\r\nmodel: opus\r\n---\r\nbody"), Ok((Some("model: opus\r\n"), "body")));
        assert_eq!(split_frontmatter("---\n---\nbody"), Ok((Some(""), "body")));
        assert_eq!(split_frontmatter("Just a prompt.\n---\n"), Ok((None, "Just a prompt.\n---\n")));
        assert!(split_frontmatter("---\nmodel: opus\nYou review.").is_err());
    }

    #[test]
    fn parses_a_role_file() {
        let (role, ignored) = parse_role(
            "reviewer",
            "---\nharness: codex\nmodel: gpt-5\neffort: ' high '\nprofile: gpt-6\nway: our-way\ndescription: |\n  Reviews\n  diffs\nsandbox: read-only\n---\n\nYou review. Never edit.\n",
        )
        .unwrap();
        assert_eq!(role.name, "reviewer");
        assert_eq!(role.harness, Harness::Codex);
        assert_eq!(role.model.as_deref(), Some("gpt-5"));
        assert_eq!(role.effort.as_deref(), Some("high"));
        assert_eq!(role.profile.as_deref(), Some("gpt-6"));
        assert_eq!(role.way.as_deref(), Some("our-way"));
        assert_eq!(role.description.as_deref(), Some("Reviews diffs"));
        assert_eq!(role.prompt.as_deref(), Some("You review. Never edit."));
        assert_eq!(ignored, vec!["sandbox".to_string()]);

        // What a Claude Code role may use: lists, trimmed, empty ones kept.
        let (limited, ignored) = parse_role(
            "lean",
            "---\nmcp: [github, ' linear ']\nplugins: []\nskills:\n  - pdf\n  - ''\ntools:\n  deny: [\"Bash(git push *)\"]\n---\n",
        )
        .unwrap();
        assert!(ignored.is_empty());
        assert_eq!(limited.mcp, Some(vec!["github".to_string(), "linear".to_string()]));
        assert_eq!(limited.plugins, Some(vec![]));
        assert_eq!(limited.skills, Some(vec!["pdf".to_string()]));
        assert_eq!(limited.tools, Some(ToolRules { allow: vec![], deny: vec!["Bash(git push *)".into()] }));
        assert_eq!((limited.prompt, limited.harness), (None, Harness::Claude));
        assert!(parse_role("x", "---\nmcp: github\n---\n").unwrap_err().starts_with("frontmatter"));

        // No frontmatter: a Claude Code role that is all prompt.
        let (plain, _) = parse_role("plain", "Be terse.").unwrap();
        assert_eq!((plain.harness, plain.prompt.as_deref(), plain.model), (Harness::Claude, Some("Be terse."), None));
        assert_eq!(plain.effort, None);
        assert_eq!(parse_role("x", "---\neffort: ''\n---\n").unwrap().0.effort, None);

        assert!(parse_role("x", "---\nharness: vim\n---\n").unwrap_err().contains("unknown harness"));
        assert!(parse_role("x", "---\nway: ../etc\n---\n").is_err());
        assert!(parse_role("x", "---\nharness: codex\nprofile: ../config\n---\n").is_err());
        assert!(parse_role("x", "---\nmodel: [opus\n---\n").unwrap_err().starts_with("frontmatter"));
    }

    fn fields_of(r: &AgentRole) -> RoleFields {
        RoleFields {
            harness: r.harness,
            model: r.model.clone(),
            effort: r.effort.clone(),
            way: r.way.clone(),
            description: r.description.clone(),
            deny: r.tools.as_ref().map(|t| t.deny.clone()).unwrap_or_default(),
            prompt: r.prompt.clone(),
        }
    }

    #[test]
    fn saves_the_forms_fields_into_a_role_file() {
        let text = "---\nharness: codex\nmodel: gpt-6-luna\nsandbox: read-only\nprofile: fast\ntools:\n  allow: [Read]\n  deny: [Edit]\n---\nYou review.\n";
        let (role, ignored) = parse_role("reviewer", text).unwrap();
        let same = set_role_fields(text, &fields_of(&role)).unwrap();
        assert_eq!(parse_role("reviewer", &same).unwrap(), (role.clone(), ignored));
        // What the form does not edit stays: an unknown key, the profile,
        // the rules it may use.
        assert_eq!(
            same,
            "---\nharness: codex\nmodel: gpt-6-luna\nsandbox: read-only\nprofile: fast\ntools:\n  allow:\n  - Read\n  deny:\n  - Edit\n---\nYou review.\n"
        );

        // Claude Code, nothing denied, no model: no keys for them.
        let plain = RoleFields { deny: vec![" ".into()], prompt: Some("You plan.".into()), ..Default::default() };
        let out = set_role_fields("---\nharness: codex\nmodel: x\ntools:\n  deny: [Edit]\n---\nOld.\n", &plain).unwrap();
        assert_eq!(out, "You plan.\n");
        let edited = RoleFields { harness: Harness::Codex, effort: Some("high".into()), deny: vec!["Bash(git push *)".into()], ..fields_of(&role) };
        let out = set_role_fields(text, &edited).unwrap();
        let (r, _) = parse_role("reviewer", &out).unwrap();
        assert_eq!((r.effort.as_deref(), r.tools.unwrap().deny), (Some("high"), vec!["Bash(git push *)".to_string()]));
    }

    #[test]
    fn a_built_in_role_survives_a_save_unchanged() {
        for (_, name, text) in crate::builtin::of("roles") {
            let parsed = parse_role(&name, text).unwrap();
            let saved = set_role_fields(text, &fields_of(&parsed.0)).unwrap();
            assert_eq!(parse_role(&name, &saved).unwrap(), parsed, "{name}");
        }
    }

    #[test]
    fn offers_what_the_launch_accepts() {
        let choices = role_choices();
        assert_eq!(choices.iter().map(|c| c.harness).collect::<Vec<_>>(), [Harness::Claude, Harness::Codex]);
        for c in &choices {
            assert_eq!(c.efforts, efforts(c.harness));
            // Every model the built-in roles run is offered.
            for (_, name, text) in crate::builtin::of("roles") {
                let (r, _) = parse_role(&name, text).unwrap();
                if r.harness == c.harness {
                    assert!(c.models.contains(r.model.as_ref().unwrap()), "{name}");
                }
            }
        }
    }

    #[test]
    fn a_name_is_safe_on_a_command_line() {
        for ok in ["reviewer", "dev-1", "lead_2", "v1.2"] {
            assert!(is_name(ok), "{ok}");
        }
        for bad in ["", "-rf", ".hidden", "two words", "a;b", "a/b", "é", &"x".repeat(65)] {
            assert!(!is_name(bad), "{bad}");
        }
    }

    #[test]
    fn the_repository_wins_over_the_user_over_the_config() {
        let root = tree("merge");
        let global = root.join("config");
        let repo = root.join("repo/.mirador");
        write(&global.join("roles/reviewer.md"), "---\nmodel: haiku\n---\nGlobal reviewer.");
        write(&global.join("roles/planner.md"), "---\nway: our-way\n---\nYou plan.");
        write(&global.join("ways/our-way.md"), "Global way.");
        write(&repo.join("roles/reviewer.md"), "---\nmodel: opus\nway: our-way\n---\nRepo reviewer.");
        write(&repo.join("ways/our-way.md"), "---\ndescription: ours\n---\nRepo way.");
        write(&repo.join("roles/notes.txt"), "not a role");
        let config = [
            AgentRole { name: "reviewer".into(), model: Some("sonnet".into()), ..Default::default() },
            AgentRole { name: "coder".into(), model: Some("sonnet".into()), ..Default::default() },
        ];

        write(&global.join("teams/squad.md"), "---\nmembers: [{role: planner, lead: true}, {role: reviewer}]\n---\nGlobal squad.");
        write(&repo.join("teams/squad.md"), "---\nway: our-way\nmembers: [{role: reviewer}, {role: coder, count: 2}]\n---\nRepo squad.");
        write(&repo.join("teams/ghosts.md"), "---\nmembers: [{role: ghost}]\n---\n");
        let cat = catalog_in(Some(&repo), &global, &config);
        // Teams merge like roles, and are checked against the merged roles.
        let teams: Vec<_> = cat
            .teams
            .iter()
            .filter(|t| t.source != RoleSource::Builtin)
            .map(|t| (t.name.as_str(), t.source, t.problem.is_some()))
            .collect();
        assert_eq!(teams, [("ghosts", RoleSource::Repo, true), ("squad", RoleSource::Repo, false)]);
        assert_eq!(cat.teams.iter().find(|t| t.name == "squad").unwrap().prompt.as_deref(), Some("Repo squad."));
        let cat = RoleCatalog { problems: cat.problems.into_iter().filter(|p| !p.contains("ghosts")).collect(), ..cat };
        assert_eq!(cat.problems, Vec::<String>::new());
        assert!(cat.roles.iter().all(|r| r.problem.is_none()));
        let names: Vec<_> = cat
            .roles
            .iter()
            .filter(|r| r.source != RoleSource::Builtin)
            .map(|r| (r.name.as_str(), r.source))
            .collect();
        assert_eq!(
            names,
            [("coder", RoleSource::Config), ("planner", RoleSource::Global), ("reviewer", RoleSource::Repo)]
        );
        let our_way = cat.ways.iter().find(|w| w.name == "our-way").unwrap();
        assert_eq!(cat.ways.iter().filter(|w| w.name == "our-way").count(), 1);
        assert_eq!((our_way.source, our_way.description.as_deref()), (RoleSource::Repo, Some("ours")));

        // The global planner follows the repository's way of the same name.
        let (role, prompt) = resolve(&cat, "reviewer").unwrap();
        assert_eq!(role.model.as_deref(), Some("opus"));
        assert_eq!(prompt.as_deref(), Some("Repo reviewer.\n\nRepo way."));
        assert_eq!(resolve(&cat, "planner").unwrap().1.as_deref(), Some("You plan.\n\nRepo way."));
        assert_eq!(resolve(&cat, "coder").unwrap().1, None);
        assert!(resolve(&cat, "nobody").is_err());

        // Outside the repository, the user's own.
        let outside = catalog_in(None, &global, &[]);
        assert_eq!(resolve(&outside, "reviewer").unwrap().1.as_deref(), Some("Global reviewer."));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lists_what_a_file_of_the_same_name_hides() {
        let root = tree("shadowed");
        let global = root.join("config");
        let repo = root.join("repo/.mirador");
        write(&global.join("roles/reviewer.md"), "Your reviewer.");
        write(&repo.join("roles/reviewer.md"), "The repo's reviewer.");
        write(&global.join("ways/our-way.md"), "Your way.");
        write(&repo.join("ways/our-way.md"), "The repo's way.");
        let config = [AgentRole { name: "reviewer".into(), ..Default::default() }];
        let cat = catalog_in(Some(&repo), &global, &config);
        let mine: Vec<_> = cat
            .shadowed
            .iter()
            .filter(|f| f.source != RoleSource::Builtin)
            .map(|f| (f.kind, f.name.as_str(), f.source, f.by))
            .collect();
        // Each hidden one names the one used, wherever it was hidden from.
        assert_eq!(
            mine,
            [
                (CatalogKind::Way, "our-way", RoleSource::Global, RoleSource::Repo),
                (CatalogKind::Role, "reviewer", RoleSource::Config, RoleSource::Repo),
                (CatalogKind::Role, "reviewer", RoleSource::Global, RoleSource::Repo),
            ]
        );
        let yours = cat.shadowed.iter().find(|f| f.source == RoleSource::Global && f.kind == CatalogKind::Role).unwrap();
        assert_eq!(yours.path.as_deref(), Some(global.join("roles/reviewer.md").to_string_lossy().as_ref()));
        assert_eq!(yours.by_path.as_deref(), Some(repo.join("roles/reviewer.md").to_string_lossy().as_ref()));
        assert_eq!(cat.shadowed.iter().find(|f| f.source == RoleSource::Config).unwrap().path, None);
        // Nothing hidden outside the repository but the mirador.json entry.
        let outside = catalog_in(None, &global, &[]);
        assert!(outside.shadowed.iter().all(|f| f.source == RoleSource::Builtin));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn says_who_uses_a_role_or_a_way() {
        let role = |name: &str, way: Option<&str>| AgentRole { name: name.into(), way: way.map(Into::into), ..Default::default() };
        let member = |role: &str| cmux_protocol::TeamMember { role: role.into(), name: None, count: 1, lead: false };
        let team = |name: &str, way: Option<&str>, members: &[&str]| AgentTeam {
            name: name.into(),
            way: way.map(Into::into),
            members: members.iter().map(|m| member(m)).collect(),
            ..Default::default()
        };
        let cat = RoleCatalog {
            roles: vec![role("dev", Some("tdd")), role("reviewer", None), role("planner", Some("tdd"))],
            teams: vec![team("squad", Some("tdd"), &["dev", "reviewer"]), team("pair", None, &["dev", "dev"])],
            ..Default::default()
        };
        let names = |u: Vec<UsedBy>| u.into_iter().map(|u| (u.kind, u.name, u.running)).collect::<Vec<_>>();
        use CatalogKind::*;
        assert_eq!(
            names(used_by(&cat, Way, "tdd", &["squad"], &["planner"])),
            [(Team, "squad".into(), true), (Role, "dev".into(), false), (Role, "planner".into(), true)]
        );
        assert_eq!(
            names(used_by(&cat, Role, "dev", &[], &[])),
            [(Team, "squad".into(), false), (Team, "pair".into(), false)]
        );
        assert_eq!(names(used_by(&cat, Role, "reviewer", &["pair"], &[])), [(Team, "squad".into(), false)]);
        assert!(used_by(&cat, Way, "nobody's", &[], &[]).is_empty());
    }

    #[test]
    fn reports_what_it_could_not_use() {
        let root = tree("problems");
        let global = root.join("config");
        write(&global.join("roles/lost.md"), "---\nway: nowhere\n---\nLost.");
        write(&global.join("roles/broken.md"), "---\nmodel: opus\n");
        write(&global.join("roles/-bad.md"), "x");
        write(&global.join("roles/gem.md"), "---\nharness: gemini\n---\n");
        write(&global.join("ways/empty.md"), "---\ndescription: nothing\n---\n\n");
        write(&global.join("roles/codex-mcp.md"), "---\nharness: codex\nmcp: [github]\n---\n");
        // Yours may allow tools; a repository's may only deny them.
        write(&global.join("roles/mine.md"), "---\ntools: { allow: [\"Bash(npm test)\"] }\n---\n");
        let repo = root.join("repo/.mirador");
        write(&repo.join("roles/theirs.md"), "---\ntools: { allow: [\"Bash(*)\"], deny: [\"Read\"] }\n---\n");
        write(&repo.join("roles/careful.md"), "---\ntools: { deny: [\"Bash(git push *)\"] }\n---\n");
        // Each harness takes its own efforts, in lower case.
        write(&global.join("roles/deep.md"), "---\neffort: ultra\n---\n");
        write(&global.join("roles/loud.md"), "---\neffort: HIGH\n---\n");
        write(&global.join("roles/codex-deep.md"), "---\nharness: codex\neffort: ultra\n---\n");
        write(&global.join("roles/codex-odd.md"), "---\nharness: codex\neffort: extreme\n---\n");
        // A Codex profile is Codex's, and yours to pick, never a repository's.
        write(&global.join("roles/yolo.md"), "---\nharness: codex\nprofile: yolo\n---\n");
        write(&global.join("roles/claude-yolo.md"), "---\nprofile: yolo\n---\n");
        write(&repo.join("roles/their-yolo.md"), "---\nharness: codex\nprofile: yolo\n---\n");

        let cat = catalog_in(Some(&repo), &global, &[]);
        let names: Vec<_> = cat.roles.iter().filter(|r| r.source != RoleSource::Builtin).map(|r| r.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "careful",
                "claude-yolo",
                "codex-deep",
                "codex-mcp",
                "codex-odd",
                "deep",
                "gem",
                "lost",
                "loud",
                "mine",
                "their-yolo",
                "theirs",
                "yolo"
            ]
        );
        let problem = |name: &str| cat.roles.iter().find(|r| r.name == name).unwrap().problem.clone();
        assert!(problem("codex-mcp").is_some_and(|p| p.contains("only Claude Code's can")));
        assert!(problem("theirs").is_some_and(|p| p.contains("cannot allow tools")));
        assert_eq!((problem("mine"), problem("careful")), (None, None));
        assert_eq!(
            problem("deep").as_deref(),
            Some("effort `ultra` is not one Claude Code takes: low, medium, high, xhigh, max")
        );
        assert!(problem("loud").is_some_and(|p| p.starts_with("effort `HIGH`")));
        assert_eq!(problem("codex-deep"), None);
        assert_eq!(
            problem("codex-odd").as_deref(),
            Some("effort `extreme` is not one Codex takes: low, medium, high, xhigh, max, ultra")
        );
        assert_eq!(problem("yolo"), None);
        assert_eq!(problem("claude-yolo").as_deref(), Some("`profile` is Codex's: a Claude Code role has none"));
        assert!(problem("their-yolo").is_some_and(|p| p.contains("cannot pick a Codex profile")));
        let has = |needle: &str| cat.problems.iter().any(|p| p.contains(needle));
        assert!(has("never closed"), "{:?}", cat.problems);
        assert!(has("a name is letters"));
        assert!(has("needs text"));
        assert!(has("`nowhere` is not in any ways/"));
        assert!(has("cannot start Gemini CLI"));
        assert!(resolve(&cat, "lost").unwrap_err().contains("nowhere"));
        let lost = cat.roles.iter().find(|r| r.name == "lost").unwrap();
        assert!(lost.problem.as_deref().is_some_and(|p| p.contains("nowhere")));
        assert!(resolve(&cat, "gem").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_prompt_file_outlives_its_launch_and_is_shared_by_its_content() {
        let dir = tree("prompts");
        let w = |role: &str, text: &str| write_launch_file(&dir, role, "md", text);
        let a = w("reviewer", "You review.").unwrap();
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "You review.");
        assert!(a.file_name().unwrap().to_string_lossy().starts_with("reviewer-"));
        // Same prompt, same file; an edited role, a new one.
        assert_eq!(w("reviewer", "You review.").unwrap(), a);
        let b = w("reviewer", "You review. Be terse.").unwrap();
        assert_ne!(a, b);
        assert!(w("a;b", "x").unwrap().file_name().unwrap().to_string_lossy().starts_with("role-"));
        let settings = write_launch_file(&dir, "reviewer", "settings.json", "{}").unwrap();
        assert!(settings.to_string_lossy().ends_with(".settings.json"));

        // Only a file unwritten for longer than the limit goes.
        let month_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(31 * 24 * 3600);
        for old in [&a, &settings] {
            std::fs::File::options().write(true).open(old).unwrap().set_modified(month_ago).unwrap();
        }
        // Unless a restored pane still names it.
        prune_prompts_in(&dir, PROMPT_MAX_AGE, std::slice::from_ref(&settings));
        assert!(!a.exists());
        assert!(b.exists());
        assert!(settings.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_the_nearest_mirador_directory() {
        let root = tree("find");
        std::fs::create_dir_all(root.join("repo/.mirador")).unwrap();
        std::fs::create_dir_all(root.join("repo/src/deep")).unwrap();
        assert_eq!(repo_dir(&root.join("repo/src/deep")), Some(root.join("repo/.mirador")));
        assert_eq!(repo_dir(&root.join("repo")), Some(root.join("repo/.mirador")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
