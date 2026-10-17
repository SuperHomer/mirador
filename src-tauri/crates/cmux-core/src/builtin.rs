//! The roles, ways of working and teams that ship with Mirador: the same
//! for everyone, read-only, and below every other place — a repository's
//! file, the user's, or an `agentRoles` entry of the same name replaces one.
//! They are Markdown like any other, compiled in from `builtin/`.

/// `(kind directory, name, text)`, as if read from `<dir>/<name>.md`.
pub const FILES: &[(&str, &str, &str)] = &[
    ("ways", "omakase", include_str!("../builtin/ways/omakase.md")),
    ("roles", "omakase-planner", include_str!("../builtin/roles/omakase-planner.md")),
    ("roles", "omakase-worker", include_str!("../builtin/roles/omakase-worker.md")),
    ("roles", "omakase-reviewer", include_str!("../builtin/roles/omakase-reviewer.md")),
    ("teams", "omakase", include_str!("../builtin/teams/omakase.md")),
];

/// How a built-in file is named where a path would be: it has none on
/// disk, and the editor reads it by this.
pub fn path(dir: &str, name: &str) -> String {
    format!("builtin:{dir}/{name}.md")
}

/// The text of the built-in file `path` names, if it is one.
pub fn read(path: &str) -> Option<&'static str> {
    let rest = path.strip_prefix("builtin:")?;
    FILES
        .iter()
        .find(|(dir, name, _)| rest == format!("{dir}/{name}.md"))
        .map(|(_, _, text)| *text)
}

/// The built-in files of one kind: `(path, name, text)`.
pub fn of(dir: &str) -> impl Iterator<Item = (String, String, &'static str)> + '_ {
    FILES
        .iter()
        .filter(move |(d, _, _)| *d == dir)
        .map(|(d, name, text)| (path(d, name), name.to_string(), *text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmux_protocol::{Harness, RoleSource};

    /// What everyone gets with no files of their own: every built-in
    /// parses, and the team can start.
    #[test]
    fn the_built_ins_parse_and_the_team_can_start() {
        let empty = std::env::temp_dir().join(format!("mira-builtin-{}", std::process::id()));
        std::fs::create_dir_all(&empty).unwrap();
        let cat = crate::roles::catalog_in(None, &empty, &[]);
        assert_eq!(cat.problems, Vec::<String>::new());
        let team = cat.teams.iter().find(|t| t.name == "omakase").expect("the omakase team");
        assert_eq!((team.source, team.problem.as_deref()), (RoleSource::Builtin, None));
        let seats: Vec<_> = crate::teams::seats(team).into_iter().map(|s| (s.name, s.lead)).collect();
        assert_eq!(seats, [("planner".into(), true), ("worker".into(), false), ("reviewer".into(), false)]);

        let role = |n: &str| cat.roles.iter().find(|r| r.name == n).unwrap().clone();
        assert_eq!(role("omakase-planner").model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(role("omakase-worker").model.as_deref(), Some("claude-opus-5-5"));
        let reviewer = role("omakase-reviewer");
        assert_eq!((reviewer.harness, reviewer.model.as_deref()), (Harness::Codex, Some("gpt-6-luna")));
        // The planner plans: its edits are off.
        assert!(role("omakase-planner").tools.unwrap().deny.contains(&"Edit".to_string()));
        assert!(cat.roles.iter().all(|r| r.source == RoleSource::Builtin && r.problem.is_none()));
        assert_eq!(read(&path("teams", "omakase")), Some(FILES[4].2));
        assert_eq!(read("builtin:../../etc/passwd"), None);
        let _ = std::fs::remove_dir_all(&empty);
    }

    /// Anyone's own file of the same name replaces a built-in.
    #[test]
    fn your_own_file_replaces_a_built_in() {
        let dir = std::env::temp_dir().join(format!("mira-builtin-own-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("roles")).unwrap();
        std::fs::write(dir.join("roles/omakase-reviewer.md"), "---\nharness: codex\nmodel: gpt-6-sol\n---\nMine.").unwrap();
        let cat = crate::roles::catalog_in(None, &dir, &[]);
        let reviewer = cat.roles.iter().find(|r| r.name == "omakase-reviewer").unwrap();
        assert_eq!((reviewer.source, reviewer.model.as_deref()), (RoleSource::Global, Some("gpt-6-sol")));
        assert_eq!(cat.roles.iter().filter(|r| r.name == "omakase-reviewer").count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
