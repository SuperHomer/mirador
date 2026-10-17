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
//! A role's frontmatter says how it starts (`harness`, `model`, `way`,
//! `description`) and its body is its prompt; a way's body is its prompt.
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

use cmux_protocol::{AgentRole, AgentWay, Harness, RoleCatalog, RoleSource};
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

    let mut roles: Vec<AgentRole> = config_roles
        .iter()
        .cloned()
        .map(|r| AgentRole { source: RoleSource::Config, path: None, ..r })
        .collect();
    let mut ways: Vec<AgentWay> = Vec::new();
    let dirs = [(Some(global), RoleSource::Global), (repo, RoleSource::Repo)];
    for (dir, source) in dirs {
        let Some(dir) = dir else { continue };
        for (path, name, text) in read_dir(&dir.join("ways"), &mut problems) {
            match parse_way(&text) {
                Ok((description, prompt)) => replace(
                    &mut ways,
                    AgentWay { name, prompt, description, source, path: display(&path) },
                    |w| &w.name,
                ),
                Err(e) => problems.push(format!("{}: {e}", display(&path))),
            }
        }
        for (path, name, text) in read_dir(&dir.join("roles"), &mut problems) {
            match parse_role(&name, &text) {
                Ok((role, ignored)) => {
                    for field in ignored {
                        problems.push(format!("{}: `{field}` is not a role field; ignored", display(&path)));
                    }
                    replace(
                        &mut roles,
                        AgentRole { source, path: Some(display(&path)), ..role },
                        |r| &r.name,
                    );
                }
                Err(e) => problems.push(format!("{}: {e}", display(&path))),
            }
        }
    }

    // A role that cannot start stays listed — it is still the role you
    // wrote — but says why, here and on the role.
    for role in &mut roles {
        role.problem = if !can_start(role.harness) {
            Some(format!("a role cannot start {} yet, only Claude Code and Codex", role.harness.label()))
        } else {
            role.way
                .as_ref()
                .filter(|way| !ways.iter().any(|w| &w.name == *way))
                .map(|way| format!("way of working `{way}` is not in any ways/ directory"))
        };
        if let Some(problem) = &role.problem {
            problems.push(format!("{}: {problem}", role.path.as_deref().unwrap_or("agentRoles")));
        }
    }
    roles.sort_by(|a, b| a.name.cmp(&b.name));
    ways.sort_by(|a, b| a.name.cmp(&b.name));
    RoleCatalog { roles, ways, problems }
}

/// The harnesses a role knows how to start: the ones whose command line
/// takes a model and a prompt Mirador knows how to give.
pub fn can_start(harness: Harness) -> bool {
    matches!(harness, Harness::Claude | Harness::Codex)
}

/// A role ready to start: the role, and its whole prompt — its own, then
/// its way of working's.
pub fn resolve(catalog: &RoleCatalog, name: &str) -> Result<(AgentRole, Option<String>), String> {
    let role = catalog
        .roles
        .iter()
        .find(|r| r.name == name)
        .ok_or_else(|| format!("no role `{name}` (`mira agent roles` lists them)"))?;
    if let Some(problem) = &role.problem {
        return Err(format!("role `{name}`: {problem}"));
    }
    let way = role.way.as_ref().and_then(|way| catalog.ways.iter().find(|w| &w.name == way));
    let prompt = [role.prompt.as_deref(), way.map(|w| w.prompt.as_str())]
        .into_iter()
        .flatten()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    Ok((role.clone(), (!prompt.is_empty()).then_some(prompt)))
}

/// Puts `item` in `list`, in place of one with the same name.
fn replace<T>(list: &mut Vec<T>, item: T, name: impl Fn(&T) -> &String) {
    match list.iter().position(|x| name(x) == name(&item)) {
        Some(i) => list[i] = item,
        None => list.push(item),
    }
}

/// The `.md` files of `dir` that can be read: path, name, text. A missing
/// directory has none, and is not a problem.
fn read_dir(dir: &Path, problems: &mut Vec<String>) -> Vec<(PathBuf, String, String)> {
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
    way: Option<String>,
    description: Option<String>,
    #[serde(flatten)]
    other: std::collections::BTreeMap<String, serde_yaml_ng::Value>,
}

/// A role file: the role, and the frontmatter keys it did not know.
fn parse_role(name: &str, text: &str) -> Result<(AgentRole, Vec<String>), String> {
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
    let role = AgentRole {
        name: name.to_string(),
        harness,
        model: present(front.model),
        prompt: present(Some(body.to_string())),
        way,
        description: present(front.description).map(|d| one_line(&d)),
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
fn parse_way(text: &str) -> Result<(Option<String>, String), String> {
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
fn split_frontmatter(text: &str) -> Result<(Option<&str>, &str), String> {
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

fn present(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Where launch prompts are written, for a harness that reads its prompt
/// from a file.
pub fn prompts_dir() -> PathBuf {
    crate::session::data_dir().join("prompts")
}

/// How long a prompt file nobody launched is kept.
pub const PROMPT_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 60 * 60);

/// Writes a launch prompt and returns where.
///
/// The file is named after the role and its content, and outlives the
/// launch: the command that names it stays in the shell's history, and
/// ↑ Enter after `/exit` is how people start an agent again — which a file
/// deleted after the launch would break. The same prompt is the same file,
/// rewritten (so not pruned) each launch; a role edited is a new one.
pub fn write_prompt(role: &str, prompt: &str) -> Result<PathBuf, String> {
    write_prompt_in(&prompts_dir(), role, prompt)
}

fn write_prompt_in(dir: &Path, role: &str, prompt: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let role = if is_name(role) { role } else { "role" };
    let path = dir.join(format!("{role}-{:016x}.md", fnv1a(prompt.as_bytes())));
    // Through a file of its own and a rename: a launch reading the same
    // prompt never sees it half written.
    let tmp = dir.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, prompt)
        .and_then(|()| std::fs::rename(&tmp, &path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("{}: {e}", path.display())
        })?;
    Ok(path)
}

/// Removes prompt files no launch has written for [`PROMPT_MAX_AGE`]:
/// run at startup.
pub fn prune_prompts() {
    prune_prompts_in(&prompts_dir(), PROMPT_MAX_AGE);
}

fn prune_prompts_in(dir: &Path, max_age: std::time::Duration) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
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
            "---\nharness: codex\nmodel: gpt-5\nway: our-way\ndescription: |\n  Reviews\n  diffs\nmcp: [github]\n---\n\nYou review. Never edit.\n",
        )
        .unwrap();
        assert_eq!(role.name, "reviewer");
        assert_eq!(role.harness, Harness::Codex);
        assert_eq!(role.model.as_deref(), Some("gpt-5"));
        assert_eq!(role.way.as_deref(), Some("our-way"));
        assert_eq!(role.description.as_deref(), Some("Reviews diffs"));
        assert_eq!(role.prompt.as_deref(), Some("You review. Never edit."));
        assert_eq!(ignored, vec!["mcp".to_string()]);

        // No frontmatter: a Claude Code role that is all prompt.
        let (plain, _) = parse_role("plain", "Be terse.").unwrap();
        assert_eq!((plain.harness, plain.prompt.as_deref(), plain.model), (Harness::Claude, Some("Be terse."), None));

        assert!(parse_role("x", "---\nharness: vim\n---\n").unwrap_err().contains("unknown harness"));
        assert!(parse_role("x", "---\nway: ../etc\n---\n").is_err());
        assert!(parse_role("x", "---\nmodel: [opus\n---\n").unwrap_err().starts_with("frontmatter"));
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

        let cat = catalog_in(Some(&repo), &global, &config);
        assert_eq!(cat.problems, Vec::<String>::new());
        assert!(cat.roles.iter().all(|r| r.problem.is_none()));
        let names: Vec<_> = cat.roles.iter().map(|r| (r.name.as_str(), r.source)).collect();
        assert_eq!(
            names,
            [("coder", RoleSource::Config), ("planner", RoleSource::Global), ("reviewer", RoleSource::Repo)]
        );
        assert_eq!(cat.ways.len(), 1);
        assert_eq!((cat.ways[0].source, cat.ways[0].description.as_deref()), (RoleSource::Repo, Some("ours")));

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
    fn reports_what_it_could_not_use() {
        let root = tree("problems");
        let global = root.join("config");
        write(&global.join("roles/lost.md"), "---\nway: nowhere\n---\nLost.");
        write(&global.join("roles/broken.md"), "---\nmodel: opus\n");
        write(&global.join("roles/-bad.md"), "x");
        write(&global.join("roles/gem.md"), "---\nharness: gemini\n---\n");
        write(&global.join("ways/empty.md"), "---\ndescription: nothing\n---\n\n");

        let cat = catalog_in(None, &global, &[]);
        let names: Vec<_> = cat.roles.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["gem", "lost"]);
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
        let a = write_prompt_in(&dir, "reviewer", "You review.").unwrap();
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "You review.");
        assert!(a.file_name().unwrap().to_string_lossy().starts_with("reviewer-"));
        // Same prompt, same file; an edited role, a new one.
        assert_eq!(write_prompt_in(&dir, "reviewer", "You review.").unwrap(), a);
        let b = write_prompt_in(&dir, "reviewer", "You review. Be terse.").unwrap();
        assert_ne!(a, b);
        assert!(write_prompt_in(&dir, "a;b", "x").unwrap().file_name().unwrap().to_string_lossy().starts_with("role-"));

        // Only a file unwritten for longer than the limit goes.
        let month_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(31 * 24 * 3600);
        std::fs::File::options().write(true).open(&a).unwrap().set_modified(month_ago).unwrap();
        prune_prompts_in(&dir, PROMPT_MAX_AGE);
        assert!(!a.exists());
        assert!(b.exists());
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
