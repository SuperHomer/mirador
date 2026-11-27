//! The OpenCode integration: a plugin in OpenCode's global plugins
//! directory that reports an OpenCode's session and turns to Mirador, as
//! Claude Code's hooks do — `mira agent-hook opencode <event> --session
//! <id>`. `mira hooks setup` and `remove` do this after Pi's extension.
//!
//! Why a TUI plugin. OpenCode 2's TUIs talk to a server, by default the
//! per-user background service (`opencode serve --service`) that all of
//! them share, and a server plugin runs there, with the environment of
//! whichever pane started the service: it cannot tell which pane an event
//! is for. A TUI plugin runs in the TUI, the `opencode` in the pane, with
//! that pane's `MIRA_PANE`. But every TUI on a server is sent every
//! session's events, so each reports only those of the session its own
//! screen shows (`ui.router.current()`), or of its subagents' sessions
//! (`data.session.root`), always as that root session: the one to resume.
//!
//! A plugin directory loads only with a server entrypoint, so ours has a
//! `server.ts` that does nothing beside the `tui.ts` that reports.

use std::path::{Path, PathBuf};

use crate::claude_integration::Action;

/// On the first line of each of the plugin's files: what makes it ours,
/// to overwrite or remove. A file without it is the person's, and kept.
const MARKER: &str = "Installed by `mira hooks setup`";

/// OpenCode's config directory: `$XDG_CONFIG_HOME/opencode`, else
/// `~/.config/opencode`, on every platform, as OpenCode reads it.
pub fn opencode_dir() -> Option<PathBuf> {
    match std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
        Some(config) => Some(PathBuf::from(config).join("opencode")),
        None => crate::config::home_dir().map(|home| PathBuf::from(home).join(".config/opencode")),
    }
}

/// The plugin's server entrypoint: nothing, but without one OpenCode
/// does not load the directory.
fn server() -> String {
    format!(
        "// {MARKER}: see tui.ts beside it.\n\nexport default {{ id: \"mira\", setup: async () => {{}} }};\n"
    )
}

/// The plugin's TUI entrypoint, calling `mira` at `mira`. OpenCode's
/// events, in `mira claude-hook`'s words: a run of the session starting is
/// `prompt-submit`, its end (succeeded, failed or interrupted) `stop`, a
/// permission asked `notification` and its answer `post-tool`.
///
/// Every TUI on a server hears every session, so each reports only the
/// session its screen shows. Runs count only for a root session: a
/// subagent's run ends inside its parent's, and reported as the root's it
/// would end the turn early. A permission is the root's whichever session
/// asks it. And once a TUI has reported work for a root, it reports that
/// root's end even after its screen has moved on, or the pane would read
/// working forever.
///
/// Nothing outside a Mirador pane. It catches everything and never makes
/// OpenCode wait: each report starts after the last ends, so they arrive
/// in order.
fn tui(mira: &Path) -> String {
    let mira = serde_json::to_string(&mira.to_string_lossy()).expect("a string serializes");
    format!(
        r#"// {MARKER}: reports this OpenCode's session and turns to Mirador.
// `mira hooks remove` deletes it; remove this line to keep it as yours.

import {{ spawn }} from "node:child_process";

const MIRA = {mira};

export default {{
  id: "mira",
  setup: async (ctx: any) => {{
    if (!process.env.MIRA_PANE) return;
    let last: Promise<unknown> = Promise.resolve();
    const report = (event: string, session: string) => {{
      last = last.then(
        () =>
          new Promise((done) => {{
            let timer: any;
            const finish = () => {{
              clearTimeout(timer);
              done(undefined);
            }};
            try {{
              const child = spawn(MIRA, ["agent-hook", "opencode", event, "--session", session], {{
                stdio: "ignore",
                windowsHide: true,
              }});
              child.on("error", finish);
              child.on("exit", finish);
              timer = setTimeout(finish, 5000);
            }} catch {{
              finish();
            }}
          }}),
      );
    }};
    const rootOf = (session: string): string => ctx.data.session.root(session);
    const shown = (): string | undefined => {{
      const route = ctx.ui.router.current();
      return route?.type === "session" ? rootOf(route.sessionID) : undefined;
    }};
    // Roots this TUI reported work for, until their run ends.
    const working = new Set<string>();
    const on = (type: string, handle: (session: string) => void) =>
      ctx.data.on(type, (e: any) => {{
        try {{
          const session = e?.data?.sessionID;
          if (session) handle(session);
        }} catch {{}}
      }});
    on("session.execution.started", (session) => {{
      if (rootOf(session) !== session || shown() !== session) return;
      working.add(session);
      report("prompt-submit", session);
    }});
    for (const end of ["session.execution.succeeded", "session.execution.failed", "session.execution.interrupted"]) {{
      on(end, (session) => {{
        if (rootOf(session) !== session) return;
        const mine = working.delete(session);
        if (mine || shown() === session) report("stop", session);
      }});
    }}
    on("permission.asked", (session) => {{
      const root = rootOf(session);
      if (shown() !== root) return;
      working.add(root);
      report("notification", root);
    }});
    on("permission.replied", (session) => {{
      const root = rootOf(session);
      if (working.has(root) || shown() === root) report("post-tool", root);
    }});
  }},
}};
"#
    )
}

/// Installs (or removes) the plugin, calling `mira` at `mira`. Only where
/// OpenCode is installed: its config directory must exist, and is never
/// created. `Repair` (what the app does by itself) leaves OpenCode alone.
/// Returns what it did, a line per change.
pub fn plugin_setup(action: Action, mira: &Path) -> Result<Vec<String>, String> {
    if action == Action::Repair {
        return Ok(Vec::new());
    }
    let dir = opencode_dir().ok_or("could not find your home directory")?;
    if !dir.is_dir() {
        return Ok(vec![format!("skipped OpenCode: no {}", dir.display())]);
    }
    plugin_at(&dir, action, mira)
}

/// The edit alone, in an explicit config directory, so it can be tested
/// without touching the developer's own OpenCode.
fn plugin_at(dir: &Path, action: Action, mira: &Path) -> Result<Vec<String>, String> {
    let plugin = dir.join("plugins/mira");
    let files = [("server.ts", server()), ("tui.ts", tui(mira))];
    // Each file: None = none, Some(false) = the person's, Some(true) = ours.
    // One that cannot be read is not known to be ours, so it is left alone.
    let mut ours = Vec::new();
    for (name, _) in &files {
        let path = plugin.join(name);
        ours.push(match std::fs::read_to_string(&path) {
            Ok(text) => Some(text.lines().next().is_some_and(|first| first.contains(MARKER))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("{}: {e}; left as it is", path.display())),
        });
    }
    // The two load as one plugin: neither is written or removed beside
    // a file of the person's own.
    if ours.contains(&Some(false)) {
        return Ok(vec![format!("kept your own {} (not installed by Mirador)", plugin.display())]);
    }
    let fail = |e: std::io::Error| format!("{}: {e}", plugin.display());
    match action {
        Action::Repair => Ok(Vec::new()),
        Action::Setup => {
            std::fs::create_dir_all(&plugin).map_err(fail)?;
            for (name, text) in &files {
                std::fs::write(plugin.join(name), text).map_err(fail)?;
            }
            Ok(vec![format!("installed the OpenCode plugin ({})", plugin.display())])
        }
        Action::Remove if ours.contains(&Some(true)) => {
            for ((name, _), mine) in files.iter().zip(&ours) {
                if *mine == Some(true) {
                    std::fs::remove_file(plugin.join(name)).map_err(fail)?;
                }
            }
            // Only if nothing else was put in it.
            let _ = std::fs::remove_dir(&plugin);
            Ok(vec![format!("removed the OpenCode plugin ({})", plugin.display())])
        }
        Action::Remove => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mira-opencode-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn mira() -> PathBuf {
        std::env::temp_dir().join("Mirador \"dev\".app/mira")
    }

    #[test]
    fn the_plugin_reports_each_event_of_the_session_it_shows() {
        let text = tui(&mira());
        assert!(text.starts_with(&format!("// {MARKER}")), "{text}");
        assert!(server().starts_with(&format!("// {MARKER}")));
        let quoted = serde_json::to_string(&mira().to_string_lossy()).unwrap();
        assert!(text.contains(&format!("const MIRA = {quoted};")), "a path with quotes stays one string:\n{text}");
        let has = |line: &str| assert!(text.contains(line), "{line}\n{text}");
        has("if (!process.env.MIRA_PANE) return;");
        // A run is reported for the root shown, and its end even once the
        // screen has moved on; a subagent's run not at all.
        has("on(\"session.execution.started\", (session) => {\n      if (rootOf(session) !== session || shown() !== session) return;\n      working.add(session);\n      report(\"prompt-submit\", session);");
        for end in ["succeeded", "failed", "interrupted"] {
            has(&format!("\"session.execution.{end}\""));
        }
        has("if (rootOf(session) !== session) return;\n        const mine = working.delete(session);\n        if (mine || shown() === session) report(\"stop\", session);");
        // A permission is the root's, whichever session asks.
        has("on(\"permission.asked\", (session) => {\n      const root = rootOf(session);\n      if (shown() !== root) return;\n      working.add(root);\n      report(\"notification\", root);");
        has("if (working.has(root) || shown() === root) report(\"post-tool\", root);");
        // A report never holds OpenCode up, nor a timer its exit.
        has("windowsHide: true");
        has("clearTimeout(timer);");
    }

    #[test]
    fn setup_writes_ours_once_and_remove_takes_only_ours() {
        let dir = temp_dir("ours");
        let plugin = dir.join("plugins/mira");
        plugin_at(&dir, Action::Setup, &mira()).unwrap();
        let once = std::fs::read_to_string(plugin.join("tui.ts")).unwrap();
        assert_eq!(once, tui(&mira()));
        assert_eq!(std::fs::read_to_string(plugin.join("server.ts")).unwrap(), server());
        plugin_at(&dir, Action::Setup, &mira()).unwrap();
        assert_eq!(std::fs::read_to_string(plugin.join("tui.ts")).unwrap(), once, "setup twice is setup once");

        let moved = std::env::temp_dir().join("elsewhere/mira");
        plugin_at(&dir, Action::Setup, &moved).unwrap();
        assert_eq!(std::fs::read_to_string(plugin.join("tui.ts")).unwrap(), tui(&moved));

        plugin_at(&dir, Action::Remove, &mira()).unwrap();
        assert!(!plugin.exists());
        assert!(dir.join("plugins").is_dir(), "the plugins directory is OpenCode's, and kept");
        assert_eq!(plugin_at(&dir, Action::Remove, &mira()).unwrap(), Vec::<String>::new(), "nothing to remove");
    }

    #[test]
    fn a_plugin_of_the_person_s_own_is_kept() {
        let dir = temp_dir("theirs");
        let plugin = dir.join("plugins/mira");
        std::fs::create_dir_all(&plugin).unwrap();
        let theirs = "export default { id: \"mine\", setup: async () => {} };\n";
        std::fs::write(plugin.join("server.ts"), theirs).unwrap();
        for action in [Action::Setup, Action::Remove] {
            let done = plugin_at(&dir, action, &mira()).unwrap();
            assert!(done[0].starts_with("kept your own"), "{done:?}");
            assert_eq!(std::fs::read_to_string(plugin.join("server.ts")).unwrap(), theirs);
            assert!(!plugin.join("tui.ts").exists(), "nothing of ours beside theirs");
        }
    }

    #[test]
    fn a_file_it_cannot_read_is_left_alone() {
        let dir = temp_dir("unreadable");
        let plugin = dir.join("plugins/mira");
        std::fs::create_dir_all(&plugin).unwrap();
        let bytes = [b"\xff\xfe // ".as_slice(), MARKER.as_bytes()].concat();
        std::fs::write(plugin.join("tui.ts"), &bytes).unwrap();
        for action in [Action::Setup, Action::Remove] {
            let err = plugin_at(&dir, action, &mira()).unwrap_err();
            assert!(err.contains("left as it is"), "{err}");
            assert_eq!(std::fs::read(plugin.join("tui.ts")).unwrap(), bytes);
        }
    }

    #[test]
    fn repair_leaves_opencode_alone() {
        let dir = temp_dir("repair");
        assert_eq!(plugin_at(&dir, Action::Repair, &mira()).unwrap(), Vec::<String>::new());
        assert!(!dir.join("plugins").exists());
    }
}
