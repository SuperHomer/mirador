//! The Claude Code integration: the hooks in `~/.claude/settings.json` that
//! light up Mirador tabs, the `/mira-diff` skill beside them, and putting
//! `mira` on PATH. Both the CLI (`mira hooks setup`, `mira install`) and the
//! app (its first-launch offer, and the repair when the app has moved) do
//! this, so it lives here and reports what it did instead of printing.
//!
//! Hooks call `mira` by its absolute path, so they work whether or not
//! `mira` is on PATH. PATH is still worth setting up — agents and the
//! `/mira-diff` skill call `mira` by name — but a hook that fires on every
//! Claude Code event must not depend on it.

use std::path::{Path, PathBuf};

/// Claude Code hook event → the `mira claude-hook` event name.
///
/// The rest only feed the agent wall's status. `UserPromptSubmit`: Claude
/// started working. `PostToolUse` and `PostToolUseFailure`: a tool ran, so
/// it went back to work after a permission prompt. `StopFailure`: the turn
/// ended on an API error, which `Stop` does not report — without it a
/// failed turn would read as working forever. The tool hooks fire on every
/// tool call, so the app answers them without looking anything up unless
/// some pane is waiting on the human.
const HOOK_EVENTS: &[(&str, &str)] = &[
    ("Notification", "notification"),
    ("Stop", "stop"),
    ("SessionStart", "session-start"),
    ("UserPromptSubmit", "prompt-submit"),
    ("PostToolUse", "post-tool"),
    ("PostToolUseFailure", "post-tool"),
    ("StopFailure", "stop-failure"),
];

/// `~/.claude`, where Claude Code keeps its settings and skills.
pub fn claude_dir() -> Option<PathBuf> {
    crate::config::home_dir().map(|home| PathBuf::from(home).join(".claude"))
}

/// The command a hook runs. Quoted, because the path can hold spaces (a
/// Windows user name, say) and Claude Code hands it to a shell.
pub fn hook_command(mira: &Path, event: &str) -> String {
    format!("\"{}\" claude-hook {event}", mira.display())
}

/// True if a hook matcher entry is one we installed: by absolute path, by
/// name (`mira claude-hook`, before hooks used a path) or as the legacy
/// `cmux claude-hook`. Setup replaces any of them; remove takes them out.
fn is_our_hook(matcher: &serde_json::Value) -> bool {
    matcher["hooks"]
        .as_array()
        .map(|hs| {
            hs.iter()
                .any(|h| h["command"].as_str().unwrap_or("").contains("claude-hook"))
        })
        .unwrap_or(false)
}

/// Our hook commands currently in the settings, one per event that has one.
fn our_commands(settings: &serde_json::Value) -> Vec<String> {
    HOOK_EVENTS
        .iter()
        .filter_map(|(event, _)| {
            settings["hooks"][*event].as_array()?.iter().find_map(|m| {
                is_our_hook(m)
                    .then(|| m["hooks"][0]["command"].as_str().map(str::to_string))
                    .flatten()
            })
        })
        .collect()
}

/// The quoted executable at the start of a hook command, if it has one.
fn quoted_executable(command: &str) -> Option<PathBuf> {
    let rest = command.strip_prefix('"')?;
    Some(PathBuf::from(&rest[..rest.find('"')?]))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HookState {
    /// None of ours.
    Missing,
    /// Ours, and they would run — this `mira`, another existing build, or
    /// the by-name form that relies on PATH.
    Installed,
    /// Ours, but calling a `mira` that no longer exists: the app moved.
    Broken,
    /// Ours and runnable, but from a version that installed fewer events
    /// than this one does.
    Outdated,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// `~/.claude` exists: Claude Code is, or has been, used here.
    pub claude_present: bool,
    pub hooks: HookState,
}

pub fn status() -> Status {
    match claude_dir() {
        Some(dir) => status_at(&dir),
        None => Status {
            claude_present: false,
            hooks: HookState::Missing,
        },
    }
}

fn status_at(claude: &Path) -> Status {
    let settings = read_settings(&claude.join("settings.json")).unwrap_or_default();
    let commands = our_commands(&settings);
    let hooks = if commands.is_empty() {
        HookState::Missing
    } else if commands
        .iter()
        .filter_map(|c| quoted_executable(c))
        .any(|exe| !exe.exists())
    {
        HookState::Broken
    } else if commands.len() < HOOK_EVENTS.len() {
        HookState::Outdated
    } else {
        HookState::Installed
    };
    Status {
        claude_present: claude.is_dir(),
        hooks,
    }
}

/// The `mira` the installed hooks call by path, when that still exists:
/// adding the events a newer version wants keeps them calling the same one.
pub fn installed_mira() -> Option<PathBuf> {
    let settings = read_settings(&claude_dir()?.join("settings.json")).ok()?;
    our_commands(&settings)
        .iter()
        .filter_map(|c| quoted_executable(c))
        .find(|exe| exe.exists())
}

fn read_settings(path: &Path) -> Result<serde_json::Value, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(_) => Ok(serde_json::json!({})),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Setup,
    Remove,
}

/// Installs (or removes) the hooks and the skill under `~/.claude`, the
/// hooks calling `mira` at `mira`. Returns what it did, a line per change.
pub fn hooks(action: Action, mira: &Path) -> Result<Vec<String>, String> {
    let claude = claude_dir().ok_or("could not find your home directory")?;
    hooks_in(&claude, action, mira)
}

fn hooks_in(claude: &Path, action: Action, mira: &Path) -> Result<Vec<String>, String> {
    let mut done = skill_at(&claude.join("skills"), action)?;
    done.extend(hooks_at(&claude.join("settings.json"), action, mira)?);
    Ok(done)
}

/// The hooks edit alone, against an explicit settings file — so it can be
/// tested without writing into the developer's own Claude Code config.
fn hooks_at(path: &Path, action: Action, mira: &Path) -> Result<Vec<String>, String> {
    let mut settings = read_settings(path)?;
    let hooks_obj = settings
        .as_object_mut()
        .ok_or("settings.json is not an object")?
        .entry("hooks")
        .or_insert(serde_json::json!({}));
    let mut done = Vec::new();

    match action {
        Action::Setup => {
            for (hook_event, cli_event) in HOOK_EVENTS {
                let entries = hooks_obj
                    .as_object_mut()
                    .ok_or("hooks is not an object")?
                    .entry(*hook_event)
                    .or_insert(serde_json::json!([]));
                let list = entries.as_array_mut().ok_or("hook entry is not an array")?;
                // Drop any prior hook of ours — by path, by name, or the old
                // `cmux` — then add this one: idempotent, and an earlier
                // install migrates to the current command.
                list.retain(|matcher| !is_our_hook(matcher));
                list.push(serde_json::json!({
                    "hooks": [{ "type": "command", "command": hook_command(mira, cli_event) }]
                }));
                done.push(format!("installed {hook_event} hook"));
            }
        }
        Action::Remove => {
            if let Some(obj) = hooks_obj.as_object_mut() {
                for (hook_event, _) in HOOK_EVENTS {
                    if let Some(list) = obj.get_mut(*hook_event).and_then(|v| v.as_array_mut()) {
                        list.retain(|matcher| !is_our_hook(matcher));
                    }
                }
                done.push("removed Mirador hooks".into());
            }
        }
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    done.push(format!("updated {}", path.display()));
    Ok(done)
}

/// The `/mira-diff` skill `mira hooks setup` installs. Agents already
/// reach the diff pane through `mira diff`; this is the human's
/// affordance, which is why it opts out of model invocation.
///
/// A skill rather than a `commands/mira-diff.md` file: Claude Code merged
/// custom commands into skills, both spellings still produce `/mira-diff`,
/// and skills are where new work is supposed to go. The `!` line needs
/// Claude Code 2.1.228 or newer to run; on anything older the skill still
/// loads, it just hands Claude the literal line instead of the output.
const SKILL_DIR: &str = "mira-diff";
const SKILL_FILE: &str = "SKILL.md";

/// Marks the file as ours. `setup` only overwrites a file carrying it and
/// `remove` only deletes one, so a hand-written skill of the same name
/// survives both — and deleting the line opts a file out of management.
const SKILL_MARKER: &str = "Installed by `mira hooks setup`";

const SKILL_BODY: &str = r#"---
name: mira-diff
description: Open a Mirador diff pane for the work in this pane
argument-hint: [commit | range | --staged | --tab]
allowed-tools: Bash(mira diff) Bash(mira diff:*)
disable-model-invocation: true
---
<!-- Installed by `mira hooks setup`, removed by `mira hooks remove`.
     Delete the line above and Mirador stops touching this file. -->

!`mira diff $ARGUMENTS`

If the command printed an error, relay it in one line. Otherwise a diff pane
is now open for review: acknowledge in one short line, and do not describe or
summarize the diff — the pane already shows it.
"#;

/// Installs (or removes) the `/mira-diff` skill under a Claude Code skills
/// directory. A file rather than a settings key, but the same rule as the
/// hooks: only ever touch what we put there.
fn skill_at(skills_dir: &Path, action: Action) -> Result<Vec<String>, String> {
    let dir = skills_dir.join(SKILL_DIR);
    let path = dir.join(SKILL_FILE);
    // None = no file, Some(false) = someone else's, Some(true) = ours.
    let ours = std::fs::read_to_string(&path)
        .ok()
        .map(|text| text.contains(SKILL_MARKER));
    let mut done = Vec::new();

    match action {
        Action::Setup => {
            if ours == Some(false) {
                done.push(format!("kept your own {} (not installed by Mirador)", path.display()));
                return Ok(done);
            }
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::write(&path, SKILL_BODY).map_err(|e| e.to_string())?;
            done.push(format!("installed /mira-diff ({})", path.display()));
        }
        Action::Remove => match ours {
            Some(true) => {
                std::fs::remove_file(&path).map_err(|e| e.to_string())?;
                // A skill is a directory; leaving an empty one behind
                // would show up as a broken `/mira-diff`. Only ours, and
                // only if nothing else was put in it.
                let _ = std::fs::remove_dir(&dir);
                done.push("removed /mira-diff".into());
            }
            Some(false) => {
                done.push(format!("kept your own {} (not installed by Mirador)", path.display()))
            }
            None => {}
        },
    }
    Ok(done)
}

/// Puts `mira` on PATH: a symlink in ~/.local/bin on unix, and on Windows
/// the install directory itself joins the user's PATH — so `mira` keeps
/// pointing at the installed app after an upgrade, and no copy goes stale.
#[cfg(unix)]
pub fn install_on_path(mira: &Path) -> Result<Vec<String>, String> {
    let home = crate::config::home_dir().ok_or("could not find your home directory")?;
    let bin_dir = PathBuf::from(home).join(".local/bin");
    std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;
    let target = bin_dir.join("mira");
    let _ = std::fs::remove_file(&target);
    std::os::unix::fs::symlink(mira, &target).map_err(|e| e.to_string())?;
    Ok(vec![
        format!("installed: {} -> {}", target.display(), mira.display()),
        "make sure ~/.local/bin is on your PATH".into(),
    ])
}

#[cfg(windows)]
pub fn install_on_path(mira: &Path) -> Result<Vec<String>, String> {
    let dir = mira
        .parent()
        .ok_or("could not determine the install directory")?
        .to_string_lossy()
        .into_owned();

    // Read/modify/write the *user* PATH through PowerShell: `setx` truncates
    // at 1024 characters, which would quietly destroy a long PATH.
    let script = format!(
        "$dir = '{}'; \
         $path = [Environment]::GetEnvironmentVariable('Path', 'User'); \
         if ($null -eq $path) {{ $path = '' }} \
         if (($path -split ';') -contains $dir) {{ 'already' }} \
         else {{ \
           $new = if ($path.TrimEnd(';')) {{ $path.TrimEnd(';') + ';' + $dir }} else {{ $dir }}; \
           [Environment]::SetEnvironmentVariable('Path', $new, 'User'); 'added' \
         }}",
        dir.replace('\'', "''")
    );
    let output = crate::proc::command("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| format!("could not update PATH: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "could not update PATH: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut done = if String::from_utf8_lossy(&output.stdout).trim() == "already" {
        vec![format!("{dir} is already on your PATH")]
    } else {
        vec![
            format!("added to your user PATH: {dir}"),
            "open a new terminal (or sign out and back in) to pick it up".into(),
        ]
    };
    done.push(format!("mira: {}", mira.display()));
    Ok(done)
}

/// Whether the user has answered the first-launch offer ("Not now" counts).
/// Kept beside the session, so a sandbox has its own.
fn offer_answered_path() -> PathBuf {
    crate::session::data_dir().join("claude-integration-offered")
}

pub fn offer_answered() -> bool {
    offer_answered_path().exists()
}

pub fn mark_offer_answered() {
    let path = offer_answered_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, b"");
}

/// The app offers the integration once: when Claude Code is used here, none
/// of our hooks are installed, and nobody has answered the offer yet.
pub fn should_offer(status: &Status, answered: bool) -> bool {
    status.claude_present && status.hooks == HookState::Missing && !answered
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mira-claude-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read(path: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// A `mira` that exists, so status reads the hooks as working.
    fn fake_mira(dir: &Path) -> PathBuf {
        let mira = dir.join(if cfg!(windows) { "mira.exe" } else { "mira" });
        std::fs::write(&mira, b"").unwrap();
        mira
    }

    #[test]
    fn hooks_call_mira_by_its_absolute_path_quoted() {
        let dir = temp_dir("abs");
        let path = dir.join("settings.json");
        let mira = dir.join("Program Files").join("mira.exe");
        hooks_at(&path, Action::Setup, &mira).unwrap();
        let settings = read(&path);
        let command = settings["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(command, format!("\"{}\" claude-hook stop", mira.display()));
        assert_eq!(quoted_executable(command), Some(mira));
    }

    #[test]
    fn setup_installs_one_hook_per_event() {
        let dir = temp_dir("events");
        let path = dir.join("settings.json");
        hooks_at(&path, Action::Setup, &fake_mira(&dir)).unwrap();
        let settings = read(&path);
        for (event, _) in HOOK_EVENTS {
            assert_eq!(settings["hooks"][*event].as_array().unwrap().len(), 1, "{event}");
        }
    }

    #[test]
    fn setup_is_idempotent_and_leaves_other_hooks_alone() {
        let dir = temp_dir("idem");
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"someone-elses-tool"}]}]},"model":"opus"}"#,
        )
        .unwrap();
        let mira = fake_mira(&dir);
        hooks_at(&path, Action::Setup, &mira).unwrap();
        hooks_at(&path, Action::Setup, &mira).unwrap();

        let settings = read(&path);
        assert_eq!(settings["model"], "opus", "unrelated settings must survive");
        let stop = settings["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "running setup twice must not duplicate ours");
        assert!(
            stop.iter().any(|m| m["hooks"][0]["command"] == "someone-elses-tool"),
            "another tool's hook must survive"
        );
    }

    #[test]
    fn remove_takes_only_our_hooks_back_out() {
        let dir = temp_dir("remove");
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"someone-elses-tool"}]}]}}"#,
        )
        .unwrap();
        let mira = fake_mira(&dir);
        hooks_at(&path, Action::Setup, &mira).unwrap();
        hooks_at(&path, Action::Remove, &mira).unwrap();

        let stop = read(&path)["hooks"]["Stop"].as_array().unwrap().clone();
        assert_eq!(stop.len(), 1);
        assert_eq!(stop[0]["hooks"][0]["command"], "someone-elses-tool");
    }

    #[test]
    fn earlier_hooks_are_migrated_not_duplicated() {
        // The by-name form, and the even older `cmux` one.
        for old in ["mira claude-hook stop", "cmux claude-hook stop"] {
            let dir = temp_dir("legacy");
            let path = dir.join("settings.json");
            std::fs::write(
                &path,
                format!(r#"{{"hooks":{{"Stop":[{{"hooks":[{{"type":"command","command":"{old}"}}]}}]}}}}"#),
            )
            .unwrap();
            let mira = fake_mira(&dir);
            hooks_at(&path, Action::Setup, &mira).unwrap();
            let stop = read(&path)["hooks"]["Stop"].as_array().unwrap().clone();
            assert_eq!(stop.len(), 1, "{old} should be replaced");
            assert_eq!(stop[0]["hooks"][0]["command"], hook_command(&mira, "stop"));
        }
    }

    #[test]
    fn status_tells_missing_installed_and_broken_apart() {
        let claude = temp_dir("status");
        assert_eq!(
            status_at(&claude),
            Status { claude_present: true, hooks: HookState::Missing }
        );

        let mira = fake_mira(&claude);
        hooks_at(&claude.join("settings.json"), Action::Setup, &mira).unwrap();
        assert_eq!(status_at(&claude).hooks, HookState::Installed);

        // An install from before the wall's events: still ours, but short.
        let path = claude.join("settings.json");
        let mut settings = read(&path);
        settings["hooks"].as_object_mut().unwrap().remove("PostToolUse");
        std::fs::write(&path, settings.to_string()).unwrap();
        assert_eq!(status_at(&claude).hooks, HookState::Outdated);
        hooks_at(&path, Action::Setup, &mira).unwrap();
        assert_eq!(status_at(&claude).hooks, HookState::Installed);

        // The app moved: the path in the hooks is gone.
        std::fs::remove_file(&mira).unwrap();
        assert_eq!(status_at(&claude).hooks, HookState::Broken);

        // The by-name form is not ours to judge: it works if mira is on PATH.
        // From before the wall, it is only short of events.
        std::fs::write(
            claude.join("settings.json"),
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"mira claude-hook stop"}]}]}}"#,
        )
        .unwrap();
        assert_eq!(status_at(&claude).hooks, HookState::Outdated);

        let nowhere = std::env::temp_dir().join(format!("mira-claude-absent-{}", std::process::id()));
        assert!(!status_at(&nowhere).claude_present);
    }

    #[test]
    fn the_offer_is_made_once_and_only_where_it_applies() {
        let present = |hooks| Status { claude_present: true, hooks };
        assert!(should_offer(&present(HookState::Missing), false));
        assert!(!should_offer(&present(HookState::Missing), true), "answered already");
        assert!(!should_offer(&present(HookState::Installed), false), "already set up");
        assert!(!should_offer(&present(HookState::Broken), false), "repaired, not offered");
        assert!(!should_offer(&present(HookState::Outdated), false), "upgraded, not offered");
        assert!(
            !should_offer(&Status { claude_present: false, hooks: HookState::Missing }, false),
            "no Claude Code here"
        );
    }

    /// A throwaway `skills/` directory (the parent the skill goes under).
    fn temp_skills(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mira-skills-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn skill_path(skills: &Path) -> PathBuf {
        skills.join(SKILL_DIR).join(SKILL_FILE)
    }

    #[test]
    fn setup_installs_the_skill_and_remove_takes_it_back() {
        let skills = temp_skills("roundtrip");
        let path = skill_path(&skills);

        skill_at(&skills, Action::Setup).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        assert!(first.contains("mira diff $ARGUMENTS"), "should call mira diff");
        assert!(
            first.contains("disable-model-invocation: true"),
            "the human types this one; agents already have `mira diff`"
        );

        skill_at(&skills, Action::Setup).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first, "not idempotent");

        skill_at(&skills, Action::Remove).unwrap();
        assert!(!path.exists(), "remove should delete our file");
        // A skill is a directory: an empty one left behind reads as a
        // broken /mira-diff in Claude Code.
        assert!(!skills.join(SKILL_DIR).exists(), "the skill dir should go too");
    }

    /// The directory cleanup must not take anything that isn't ours.
    #[test]
    fn remove_keeps_a_skill_directory_that_holds_other_files() {
        let skills = temp_skills("companion");
        skill_at(&skills, Action::Setup).unwrap();
        let theirs = skills.join(SKILL_DIR).join("notes.md");
        std::fs::write(&theirs, "mine").unwrap();

        skill_at(&skills, Action::Remove).unwrap();

        assert!(!skill_path(&skills).exists(), "ours should go");
        assert!(theirs.exists(), "a file someone else put there must survive");
    }

    /// A user's own `/mira-diff` must survive both actions. Without the
    /// marker check, setup would silently overwrite a command someone wrote
    /// themselves — and remove would delete it.
    #[test]
    fn a_hand_written_skill_of_the_same_name_is_never_touched() {
        let skills = temp_skills("foreign");
        let path = skill_path(&skills);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mine = "---\ndescription: my own thing\n---\n!`echo hi`\n";
        std::fs::write(&path, mine).unwrap();

        skill_at(&skills, Action::Setup).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), mine, "setup clobbered it");

        skill_at(&skills, Action::Remove).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), mine, "remove deleted it");
    }

    /// Removing from a directory that never had the command is a no-op,
    /// not an error — `hooks remove` runs on machines that never ran setup.
    #[test]
    fn remove_without_an_install_is_quiet() {
        let skills = temp_skills("absent");
        skill_at(&skills, Action::Remove).unwrap();
        assert!(!skill_path(&skills).exists());
    }

    #[test]
    fn hooks_in_does_both_the_skill_and_the_settings() {
        let claude = temp_dir("both");
        let mira = fake_mira(&claude);
        let done = hooks_in(&claude, Action::Setup, &mira).unwrap();
        assert!(done.iter().any(|l| l.starts_with("installed /mira-diff")));
        assert!(skill_path(&claude.join("skills")).exists());
        assert_eq!(status_at(&claude).hooks, HookState::Installed);
    }
}
