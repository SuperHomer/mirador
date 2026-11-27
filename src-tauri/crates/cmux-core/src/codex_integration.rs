//! The Codex integration: Mirador's `notify` in the person's Codex
//! `config.toml`, so that a Codex started by hand in a pane reports its
//! turns as one Mirador starts does (which gets it as `-c notify=[…]`).
//! `mira hooks setup` and `remove` do this after Claude Code's hooks.
//!
//! Codex's own hooks (`hooks.json`) are not used: a hook the person adds
//! there needs a trust hash, or Codex asks "Trust all?" at every start.
//! Its older `notify` runs after each turn, with the turn's thread.
//!
//! The person's own notify keeps running: ours runs it after `--`, and
//! removing ours puts it back. The file is edited with `toml_edit`, so its
//! comments, order and layout survive; one that does not parse is left as
//! it is.

use std::path::{Path, PathBuf};

use crate::agents::{codex_notify_argv, codex_notify_inner};
use crate::claude_integration::Action;

/// Where Codex keeps its config: `$CODEX_HOME`, else `~/.codex`.
pub fn codex_dir() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .or_else(|| crate::config::home_dir().map(|home| PathBuf::from(home).join(".codex")))
}

/// Sets up (or removes) Mirador's notify in Codex's config, calling `mira`
/// at `mira`. Only where Codex is installed: its directory is never
/// created. `Repair` (what the app does by itself) leaves Codex alone.
/// Returns what it did, a line per change.
pub fn notify(action: Action, mira: &Path) -> Result<Vec<String>, String> {
    if action == Action::Repair {
        return Ok(Vec::new());
    }
    let dir = codex_dir().ok_or("could not find your home directory")?;
    if !dir.is_dir() {
        return Ok(vec![format!("skipped Codex: no {}", dir.display())]);
    }
    notify_at(&dir.join("config.toml"), action, mira)
}

/// The notify edit alone, against an explicit config file, so it can be
/// tested without touching the developer's own Codex config.
fn notify_at(path: &Path, action: Action, mira: &Path) -> Result<Vec<String>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let left = |why: String| format!("{}: {why}; left as it is", path.display());
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| left(format!("not TOML: {e}")))?;
    let current = match doc.get("notify") {
        None => None,
        Some(item) => Some(strings(item).ok_or_else(|| left("its notify is not a list of strings".into()))?),
    };
    let ours = current.as_deref().and_then(codex_notify_inner);

    let (wanted, done) = match action {
        Action::Repair => return Ok(Vec::new()),
        Action::Setup => (Some(codex_notify_argv(mira, current.clone())), "set up Codex notify"),
        Action::Remove => match ours {
            None => return Ok(vec![format!("no Mirador notify in {}", path.display())]),
            Some(theirs) => (Some(theirs).filter(|t| !t.is_empty()), "removed Codex notify"),
        },
    };
    if wanted == current {
        return Ok(vec![format!("Codex notify already set up in {}", path.display())]);
    }
    match wanted {
        Some(argv) => {
            let array: toml_edit::Array = argv.iter().map(String::as_str).collect();
            match doc.get_mut("notify").and_then(|item| item.as_value_mut()) {
                // Replaced in place: the key keeps its spot and spacing.
                Some(value) => {
                    let decor = value.decor().clone();
                    *value = array.into();
                    *value.decor_mut() = decor;
                }
                None => {
                    doc.insert("notify", toml_edit::value(array));
                }
            }
        }
        None => {
            doc.remove("notify");
        }
    }
    std::fs::write(path, doc.to_string()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(vec![done.to_string(), format!("updated {}", path.display())])
}

/// `item` as a list of strings, as Codex reads `notify`.
fn strings(item: &toml_edit::Item) -> Option<Vec<String>> {
    item.as_array()?.iter().map(|v| v.as_str().map(String::from)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config(name: &str, text: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mira-codex-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        if let Some(text) = text {
            std::fs::write(&path, text).unwrap();
        }
        path
    }

    fn mira() -> PathBuf {
        std::env::temp_dir().join("Mirador.app/Contents/MacOS/mira")
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    fn notify_of(path: &Path) -> Option<Vec<String>> {
        let doc: toml_edit::DocumentMut = read(path).parse().unwrap();
        doc.get("notify").and_then(strings)
    }

    const CONFIG: &str = "# my Codex\nmodel = \"gpt-5\"   # the good one\n\n[tui]\nnotifications = true\n";

    #[test]
    fn setup_adds_ours_once_and_keeps_the_rest_byte_for_byte() {
        let path = temp_config("setup", Some(CONFIG));
        let mine = mira().to_string_lossy().into_owned();
        notify_at(&path, Action::Setup, &mira()).unwrap();
        let once = read(&path);
        assert_eq!(notify_of(&path), Some(vec![mine, "codex-notify".into()]));
        // Top-level, before the first table, everything else as it was.
        let (before, tables) = once.split_at(once.find("[tui]").unwrap());
        assert!(before.starts_with("# my Codex\nmodel = \"gpt-5\"   # the good one\n"), "{once}");
        assert!(before.contains("notify = ["), "{once}");
        assert_eq!(tables, "[tui]\nnotifications = true\n");

        let again = notify_at(&path, Action::Setup, &mira()).unwrap();
        assert_eq!(read(&path), once, "setup twice is setup once");
        assert!(again[0].contains("already"), "{again:?}");

        notify_at(&path, Action::Remove, &mira()).unwrap();
        assert_eq!(read(&path), CONFIG, "remove puts the file back");
    }

    #[test]
    fn wraps_their_notify_and_puts_it_back_exactly() {
        let text = "notify = [\"/bin/sh\", \"-c\", \"say 'done'\"] # theirs\nmodel = \"gpt-5\"\n";
        let path = temp_config("theirs", Some(text));
        let theirs = vec!["/bin/sh".to_string(), "-c".into(), "say 'done'".into()];
        notify_at(&path, Action::Setup, &mira()).unwrap();
        let mut wrapped = vec![mira().to_string_lossy().into_owned(), "codex-notify".into(), "--".into()];
        wrapped.extend(theirs.clone());
        assert_eq!(notify_of(&path), Some(wrapped.clone()));
        assert!(read(&path).ends_with(" # theirs\nmodel = \"gpt-5\"\n"), "{}", read(&path));

        // A moved app re-points ours, still wrapping theirs, not ours.
        let moved = std::env::temp_dir().join("elsewhere/mira");
        notify_at(&path, Action::Setup, &moved).unwrap();
        wrapped[0] = moved.to_string_lossy().into_owned();
        assert_eq!(notify_of(&path), Some(wrapped));

        notify_at(&path, Action::Remove, &mira()).unwrap();
        assert_eq!(notify_of(&path), Some(theirs));
        assert!(read(&path).ends_with(" # theirs\nmodel = \"gpt-5\"\n"), "{}", read(&path));
    }

    #[test]
    fn remove_without_ours_changes_nothing() {
        let text = "notify = [\"notify-send\", \"Codex\"]\n# keep\n";
        for (name, text) in [("none", CONFIG), ("theirs-only", text)] {
            let path = temp_config(name, Some(text));
            let done = notify_at(&path, Action::Remove, &mira()).unwrap();
            assert_eq!(read(&path), text, "{done:?}");
        }
        let path = temp_config("no-file", None);
        notify_at(&path, Action::Remove, &mira()).unwrap();
        assert!(!path.exists(), "no file made to remove nothing from");
    }

    #[test]
    fn a_config_it_cannot_read_is_left_alone() {
        for (name, text) in [("broken", "model = \"gpt-5\nnotify = [\n"), ("not-a-list", "notify = \"say\"\n")] {
            let path = temp_config(name, Some(text));
            for action in [Action::Setup, Action::Remove] {
                let err = notify_at(&path, action, &mira()).unwrap_err();
                assert!(err.contains("left as it is"), "{err}");
                assert_eq!(read(&path), text);
            }
        }
    }

    #[test]
    fn setup_with_no_config_yet_writes_only_ours() {
        let path = temp_config("fresh", None);
        notify_at(&path, Action::Setup, &mira()).unwrap();
        assert_eq!(notify_of(&path), Some(vec![mira().to_string_lossy().into_owned(), "codex-notify".into()]));
        assert_eq!(read(&path).lines().count(), 1, "{}", read(&path));
    }

    #[test]
    fn repair_leaves_codex_alone() {
        let path = temp_config("repair", Some(CONFIG));
        assert_eq!(notify_at(&path, Action::Repair, &mira()).unwrap(), Vec::<String>::new());
        assert_eq!(read(&path), CONFIG);
    }
}
