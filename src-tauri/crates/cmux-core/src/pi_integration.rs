//! The Pi integration: an extension in Pi's global extensions directory
//! that reports a Pi's session and turns to Mirador, as Claude Code's hooks
//! do — `mira agent-hook pi <event> --session <id>`. `mira hooks setup`
//! and `remove` do this after Claude Code's hooks and Codex's notify.
//!
//! Pi has no hooks to configure, only extensions: TypeScript modules it
//! loads into its own process (through jiti) from `extensions/` in its
//! agent directory. An extension that throws or waits would be Pi's
//! problem, so this one catches everything and never waits on `mira`.

use std::path::{Path, PathBuf};

use crate::claude_integration::Action;

/// In the extension's first line: what makes a file ours, to overwrite or
/// remove. A file without it is the person's, and kept.
const MARKER: &str = "Installed by `mira hooks setup`";

const FILE: &str = "mira.ts";

/// Pi's agent directory: `$PI_CODING_AGENT_DIR` (a leading `~` is the
/// home, as Pi reads it), else `~/.pi/agent`.
pub fn pi_dir() -> Option<PathBuf> {
    let home = crate::config::home_dir().map(PathBuf::from);
    match std::env::var("PI_CODING_AGENT_DIR").ok().filter(|d| !d.is_empty()) {
        Some(dir) if dir == "~" => home,
        Some(dir) => match dir.strip_prefix("~/") {
            Some(rest) => home.map(|h| h.join(rest)),
            None => Some(PathBuf::from(dir)),
        },
        None => home.map(|h| h.join(".pi/agent")),
    }
}

/// The extension, calling `mira` at `mira`. Pi's events, in `mira
/// claude-hook`'s words: `session_start` a session, `agent_start` work,
/// `agent_settled` the turn's end — not `agent_end`, after which Pi may
/// still retry or go on with queued work. Pi asks no permissions, so it
/// has nothing for `notification`.
///
/// Nothing outside a Mirador pane, which has no tab to light. Nothing in
/// the factory either: Pi loads extensions for commands that start no
/// session. Each report waits for the last, so they arrive in order, but
/// Pi never waits for one.
pub fn extension(mira: &Path) -> String {
    let mira = serde_json::to_string(&mira.to_string_lossy()).expect("a string serializes");
    format!(
        r#"// {MARKER}: reports this Pi's session and turns to Mirador.
// `mira hooks remove` deletes it; remove this line to keep it as yours.

const MIRA = {mira};

export default function (pi: any) {{
  if (!process.env.MIRA_PANE) return;
  let last: Promise<unknown> = Promise.resolve();
  const report = (event: string, ctx: any) => {{
    try {{
      const id = String(ctx.sessionManager.getSessionId());
      last = last
        .then(() => pi.exec(MIRA, ["agent-hook", "pi", event, "--session", id], {{ timeout: 5000 }}))
        .catch(() => {{}});
    }} catch {{}}
  }};
  pi.on("session_start", (_event: unknown, ctx: any) => {{ report("session-start", ctx); }});
  pi.on("agent_start", (_event: unknown, ctx: any) => {{ report("prompt-submit", ctx); }});
  pi.on("agent_settled", (_event: unknown, ctx: any) => {{ report("stop", ctx); }});
}}
"#
    )
}

/// Installs (or removes) the extension, calling `mira` at `mira`. Only
/// where Pi is installed: its agent directory must exist, and is never
/// created. `Repair` (what the app does by itself) leaves Pi alone.
/// Returns what it did, a line per change.
pub fn extension_setup(action: Action, mira: &Path) -> Result<Vec<String>, String> {
    if action == Action::Repair {
        return Ok(Vec::new());
    }
    let dir = pi_dir().ok_or("could not find your home directory")?;
    if !dir.is_dir() {
        return Ok(vec![format!("skipped Pi: no {}", dir.display())]);
    }
    extension_at(&dir, action, mira)
}

/// The edit alone, in an explicit agent directory, so it can be tested
/// without touching the developer's own Pi.
fn extension_at(dir: &Path, action: Action, mira: &Path) -> Result<Vec<String>, String> {
    let path = dir.join("extensions").join(FILE);
    let fail = |e: std::io::Error| format!("{}: {e}", path.display());
    // None = no file, Some(false) = the person's, Some(true) = ours. One
    // that cannot be read is not known to be ours, so it is left alone.
    let ours = match std::fs::read_to_string(&path) {
        Ok(text) => Some(text.lines().next().is_some_and(|first| first.contains(MARKER))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("{}: {e}; left as it is", path.display())),
    };
    if ours == Some(false) {
        return Ok(vec![format!("kept your own {} (not installed by Mirador)", path.display())]);
    }
    match action {
        Action::Repair => Ok(Vec::new()),
        Action::Setup => {
            std::fs::create_dir_all(path.parent().expect("in extensions/")).map_err(fail)?;
            std::fs::write(&path, extension(mira)).map_err(fail)?;
            Ok(vec![format!("installed the Pi extension ({})", path.display())])
        }
        Action::Remove if ours == Some(true) => {
            std::fs::remove_file(&path).map_err(fail)?;
            Ok(vec![format!("removed the Pi extension ({})", path.display())])
        }
        Action::Remove => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mira-pi-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn mira() -> PathBuf {
        std::env::temp_dir().join("Mirador \"dev\".app/mira")
    }

    #[test]
    fn the_extension_calls_mira_by_its_path_for_each_event() {
        let text = extension(&mira());
        assert!(text.starts_with(&format!("// {MARKER}")), "{text}");
        let quoted = serde_json::to_string(&mira().to_string_lossy()).unwrap();
        assert!(text.contains(&format!("const MIRA = {quoted};")), "a path with quotes stays one string:\n{text}");
        for (pi_event, hook_event) in [("session_start", "session-start"), ("agent_start", "prompt-submit"), ("agent_settled", "stop")] {
            assert!(text.contains(&format!("pi.on(\"{pi_event}\", (_event: unknown, ctx: any) => {{ report(\"{hook_event}\", ctx); }});")), "{pi_event}");
        }
        assert!(text.contains("if (!process.env.MIRA_PANE) return;"), "nothing outside a Mirador pane");
    }

    #[test]
    fn setup_writes_ours_once_and_remove_takes_only_ours() {
        let dir = temp_dir("ours");
        let path = dir.join("extensions/mira.ts");
        extension_at(&dir, Action::Setup, &mira()).unwrap();
        let once = std::fs::read_to_string(&path).unwrap();
        assert_eq!(once, extension(&mira()));
        extension_at(&dir, Action::Setup, &mira()).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), once, "setup twice is setup once");

        // A moved app re-points it.
        let moved = std::env::temp_dir().join("elsewhere/mira");
        extension_at(&dir, Action::Setup, &moved).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), extension(&moved));

        extension_at(&dir, Action::Remove, &mira()).unwrap();
        assert!(!path.exists());
        assert!(dir.join("extensions").is_dir(), "the directory is Pi's, and kept");
        assert_eq!(extension_at(&dir, Action::Remove, &mira()).unwrap(), Vec::<String>::new(), "nothing to remove");
    }

    #[test]
    fn a_mira_ts_of_the_person_s_own_is_kept() {
        let dir = temp_dir("theirs");
        let path = dir.join("extensions/mira.ts");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let theirs = "export default function (pi) {}\n";
        std::fs::write(&path, theirs).unwrap();
        for action in [Action::Setup, Action::Remove] {
            let done = extension_at(&dir, action, &mira()).unwrap();
            assert!(done[0].starts_with("kept your own"), "{done:?}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), theirs);
        }
    }

    #[test]
    fn a_mira_ts_it_cannot_read_is_left_alone() {
        let dir = temp_dir("unreadable");
        let path = dir.join("extensions/mira.ts");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Not UTF-8, with our marker further on: still not known to be ours.
        let bytes = [b"\xff\xfe // ".as_slice(), MARKER.as_bytes()].concat();
        std::fs::write(&path, &bytes).unwrap();
        for action in [Action::Setup, Action::Remove] {
            let err = extension_at(&dir, action, &mira()).unwrap_err();
            assert!(err.contains("left as it is"), "{err}");
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        // The marker counts only on the first line.
        std::fs::write(&path, format!("export default function () {{}}\n// {MARKER}\n")).unwrap();
        let done = extension_at(&dir, Action::Remove, &mira()).unwrap();
        assert!(done[0].starts_with("kept your own"), "{done:?}");
    }

    #[test]
    fn repair_leaves_pi_alone() {
        let dir = temp_dir("repair");
        assert_eq!(extension_at(&dir, Action::Repair, &mira()).unwrap(), Vec::<String>::new());
        assert!(!dir.join("extensions").exists());
    }
}
