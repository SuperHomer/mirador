//! Editing the role, way and team files from the app: reading, checking,
//! saving, creating and deleting them.
//!
//! The editor runs in the privileged webview, so every path it hands back
//! is checked here: an `.md` file directly in a `roles/`, `ways/` or
//! `teams/` directory of the repository's `.mirador` or of the config
//! directory — nothing else is read, written or deleted, whatever the page
//! asks. And nothing that does not parse is saved: a role that breaks on
//! its next launch is worse than an edit that waits.

use std::path::{Path, PathBuf};

use cmux_protocol::RoleSource;

/// What a file defines, and the directory it lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Role,
    Way,
    Team,
}

impl Kind {
    pub fn parse(s: &str) -> Result<Kind, String> {
        match s {
            "role" => Ok(Kind::Role),
            "way" => Ok(Kind::Way),
            "team" => Ok(Kind::Team),
            other => Err(format!("`{other}` is not role, way or team")),
        }
    }

    fn dir(self) -> &'static str {
        match self {
            Kind::Role => "roles",
            Kind::Way => "ways",
            Kind::Team => "teams",
        }
    }

    fn of_dir(name: &str) -> Option<Kind> {
        match name {
            "roles" => Some(Kind::Role),
            "ways" => Some(Kind::Way),
            "teams" => Some(Kind::Team),
            _ => None,
        }
    }
}

/// The two places files live, as seen from one directory: the repository's
/// `.mirador` (the nearest one, else one to create at `repo_root`), and the
/// user's config directory.
#[derive(Debug, Clone)]
pub struct Places {
    pub repo: Option<PathBuf>,
    pub global: PathBuf,
}

impl Places {
    /// For a pane in `cwd`, in the repository rooted at `repo_root` (which
    /// may have no `.mirador` yet).
    pub fn of(cwd: Option<&Path>, repo_root: Option<&Path>) -> Places {
        let repo = cwd
            .and_then(crate::roles::repo_dir)
            .or_else(|| repo_root.map(|r| r.join(".mirador")));
        Places { repo, global: crate::config::config_dir() }
    }

    fn base(&self, source: RoleSource) -> Result<&Path, String> {
        match source {
            RoleSource::Repo => self
                .repo
                .as_deref()
                .ok_or_else(|| "this pane is not in a repository: save it in yours instead".to_string()),
            RoleSource::Global => Ok(&self.global),
            RoleSource::Config => Err("an `agentRoles` entry is edited in mirador.json".into()),
            RoleSource::Builtin => Err("a built-in ships with Mirador: save a copy in yours or the repository".into()),
        }
    }

    /// `path`, if it is one the editor may touch: what it defines, and
    /// where. Compared after resolving links, so `roles/../../x.md` or a
    /// symlinked `roles` leading elsewhere is refused.
    pub fn check_path(&self, path: &Path) -> Result<(Kind, RoleSource), String> {
        let refuse = || format!("{} is not a role, way or team file", path.display());
        if path.extension().is_none_or(|e| e != "md") {
            return Err(refuse());
        }
        let name = path.file_stem().and_then(|s| s.to_str()).ok_or_else(refuse)?;
        if !crate::roles::is_name(name) {
            return Err(refuse());
        }
        let dir = path.parent().ok_or_else(refuse)?;
        let kind = dir.file_name().and_then(|n| n.to_str()).and_then(Kind::of_dir).ok_or_else(refuse)?;
        let base = dir.parent().ok_or_else(refuse)?;
        let real = |p: &Path| p.canonicalize().ok();
        let base_real = real(base).ok_or_else(refuse)?;
        // The directory itself must resolve to `<base>/<kind dir>`: a
        // symlinked `roles` pointing elsewhere is not that.
        if real(dir) != Some(base_real.join(kind.dir())) {
            return Err(refuse());
        }
        if path.exists() && real(path) != Some(base_real.join(kind.dir()).join(format!("{name}.md"))) {
            return Err(refuse());
        }
        if self.repo.as_deref().and_then(real) == Some(base_real.clone()) {
            Ok((kind, RoleSource::Repo))
        } else if real(&self.global) == Some(base_real) {
            Ok((kind, RoleSource::Global))
        } else {
            Err(refuse())
        }
    }

    /// The text of the file at `path` — or of a built-in, by its
    /// `builtin:` name.
    pub fn read(&self, path: &Path) -> Result<String, String> {
        if let Some(text) = path.to_str().and_then(crate::builtin::read) {
            return Ok(text.to_string());
        }
        self.check_path(path)?;
        std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Saves `text` as `name`, a `kind`, in `source`'s place — a new file
    /// when `create`, which must not exist; else one that does. Refused,
    /// and nothing written, if it does not parse. Returns its path.
    pub fn save(&self, kind: Kind, source: RoleSource, name: &str, text: &str, create: bool) -> Result<PathBuf, String> {
        if !crate::roles::is_name(name) {
            return Err(format!(
                "`{name}`: a name is letters, digits, `-`, `_` and `.`, not starting with `-` or `.`"
            ));
        }
        check(kind, name, text)?;
        let dir = self.base(source)?.join(kind.dir());
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(format!("{name}.md"));
        self.check_path(&path)?;
        match (create, path.exists()) {
            (true, true) => return Err(format!("{} already exists", path.display())),
            (false, false) => return Err(format!("{} is gone: it was deleted or renamed", path.display())),
            _ => {}
        }
        let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
        std::fs::write(&tmp, text)
            .and_then(|()| std::fs::rename(&tmp, &path))
            .map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                format!("{}: {e}", path.display())
            })?;
        Ok(path)
    }

    /// Deletes the file at `path`.
    pub fn delete(&self, path: &Path) -> Result<(), String> {
        self.check_path(path)?;
        std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// What is wrong with `text` as a `kind` called `name`: an error if it does
/// not parse (it cannot be saved), else its warnings — fields that are not
/// fields — which do not stop a save. Whether its roles and ways exist is
/// the catalog's to say, once it is saved.
pub fn check(kind: Kind, name: &str, text: &str) -> Result<Vec<String>, String> {
    let ignored = match kind {
        Kind::Role => crate::roles::parse_role(name, text)?.1,
        Kind::Way => {
            crate::roles::parse_way(text)?;
            Vec::new()
        }
        Kind::Team => crate::teams::parse_team(name, Path::new(""), RoleSource::Global, text)?.1,
    };
    Ok(ignored.into_iter().map(|f| format!("`{f}` is not a field; it is ignored")).collect())
}

/// A new file's starting text: every field, commented where optional.
pub fn template(kind: Kind) -> &'static str {
    match kind {
        Kind::Role => {
            "---\n\
             description: What this agent is for, in one line\n\
             # harness: codex          # Claude Code unless set\n\
             # model: sonnet\n\
             # effort: high            # how hard the model thinks\n\
             # profile: yolo           # Codex only: ~/.codex/yolo.config.toml\n\
             # way: our-way            # a way of working, appended to this prompt\n\
             # skills: [pdf]           # only these; [] for none\n\
             # mcp: [github]\n\
             # plugins: []\n\
             # tools:\n\
             #   deny: [\"Bash(git push *)\"]\n\
             ---\n\
             You are … Describe the role: what it does, what it never does.\n"
        }
        Kind::Way => {
            "---\n\
             description: How we work, in one line\n\
             ---\n\
             1. …\n\
             2. …\n"
        }
        Kind::Team => {
            "---\n\
             description: What this team is for, in one line\n\
             # way: our-way\n\
             # layout: grid             # or columns, rows\n\
             members:\n\
             \x20 - role: planner\n\
             \x20   lead: true            # gets the team's task\n\
             \x20 - role: dev\n\
             \x20   count: 2              # dev-1, dev-2\n\
             ---\n\
             The team's charter: who does what, and how work moves between them.\n"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn places(name: &str) -> (PathBuf, Places) {
        let root = std::env::temp_dir().join(format!("mira-role-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("repo/.mirador")).unwrap();
        std::fs::create_dir_all(root.join("config")).unwrap();
        let places = Places { repo: Some(root.join("repo/.mirador")), global: root.join("config") };
        (root, places)
    }

    #[test]
    fn saves_reads_and_deletes_only_role_files() {
        let (root, p) = places("save");
        let path = p.save(Kind::Role, RoleSource::Repo, "reviewer", "---\nmodel: haiku\n---\nYou review.\n", true).unwrap();
        assert_eq!(path, root.join("repo/.mirador/roles/reviewer.md"));
        assert_eq!(p.read(&path).unwrap(), "---\nmodel: haiku\n---\nYou review.\n");
        assert_eq!(p.check_path(&path).unwrap(), (Kind::Role, RoleSource::Repo));
        // Creating over it, or saving one that is gone, is refused.
        assert!(p.save(Kind::Role, RoleSource::Repo, "reviewer", "x", true).unwrap_err().contains("already exists"));
        p.save(Kind::Role, RoleSource::Repo, "reviewer", "Edited.", false).unwrap();
        assert_eq!(p.read(&path).unwrap(), "Edited.");
        let team = p.save(Kind::Team, RoleSource::Global, "squad", template(Kind::Team), true).unwrap();
        assert_eq!(p.check_path(&team).unwrap(), (Kind::Team, RoleSource::Global));
        p.delete(&path).unwrap();
        assert!(!path.exists());
        assert!(p.save(Kind::Role, RoleSource::Repo, "reviewer", "x", false).unwrap_err().contains("is gone"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_anything_but_a_role_way_or_team_file() {
        let (root, p) = places("refuse");
        std::fs::write(root.join("repo/secret.md"), "s").unwrap();
        std::fs::create_dir_all(root.join("repo/.mirador/roles")).unwrap();
        std::fs::write(root.join("repo/.mirador/roles/notes.txt"), "n").unwrap();
        std::fs::create_dir_all(root.join("elsewhere/roles")).unwrap();
        std::fs::write(root.join("elsewhere/roles/x.md"), "x").unwrap();
        for bad in [
            root.join("repo/secret.md"),
            root.join("repo/.mirador/roles/notes.txt"),
            root.join("repo/.mirador/roles/../../secret.md"),
            root.join("repo/.mirador/other/x.md"),
            root.join("elsewhere/roles/x.md"),
            PathBuf::from("/etc/passwd"),
        ] {
            assert!(p.check_path(&bad).is_err(), "{}", bad.display());
            assert!(p.read(&bad).is_err());
            assert!(p.delete(&bad).is_err());
        }
        assert!(root.join("repo/secret.md").exists());
        // A `roles` that is a link to somewhere else is not the roles dir.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("elsewhere/roles"), root.join("config/roles")).unwrap();
            assert!(p.check_path(&root.join("config/roles/x.md")).is_err());
            assert!(p.save(Kind::Role, RoleSource::Global, "y", "Y.", true).is_err());
            assert!(!root.join("elsewhere/roles/y.md").exists());
        }
        // Names that are not names, and mirador.json's roles.
        assert!(p.save(Kind::Role, RoleSource::Repo, "../x", "x", true).is_err());
        assert!(p.save(Kind::Role, RoleSource::Config, "x", "x", true).is_err());
        assert!(p.save(Kind::Role, RoleSource::Builtin, "x", "x", false).is_err());
        // A built-in reads by its name, and is never a file to delete.
        assert!(p.read(Path::new("builtin:teams/omakase.md")).unwrap().contains("omakase"));
        assert!(p.delete(Path::new("builtin:teams/omakase.md")).is_err());
        assert!(p.read(Path::new("builtin:../secret.md")).is_err());
        let nowhere = Places { repo: None, global: p.global.clone() };
        assert!(nowhere.save(Kind::Way, RoleSource::Repo, "w", "W.", true).unwrap_err().contains("not in a repository"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn checks_before_it_saves() {
        let (root, p) = places("check");
        assert_eq!(check(Kind::Role, "r", "---\nmodel: haiku\nsandbox: x\n---\nR.").unwrap(), ["`sandbox` is not a field; it is ignored"]);
        assert!(check(Kind::Role, "r", "---\nmodel: [haiku\n---\n").is_err());
        assert!(check(Kind::Way, "w", "---\n---\n").unwrap_err().contains("needs text"));
        assert!(check(Kind::Team, "t", "---\nmembers: planner\n---\n").is_err());
        // Every template is a valid file of its kind.
        for kind in [Kind::Role, Kind::Way, Kind::Team] {
            assert_eq!(check(kind, "x", template(kind)), Ok(vec![]), "{kind:?}");
        }
        // A broken edit leaves the file as it was.
        let path = p.save(Kind::Way, RoleSource::Repo, "our-way", "Tests first.", true).unwrap();
        assert!(p.save(Kind::Way, RoleSource::Repo, "our-way", "---\nbroken", false).is_err());
        assert_eq!(p.read(&path).unwrap(), "Tests first.");
        assert_eq!(Kind::parse("team"), Ok(Kind::Team));
        assert!(Kind::parse("roles").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
