//! Checkpoints: a team's conversations, or one agent's, saved under a name
//! to be picked up again after the computer was turned off — `mira team
//! save` / `team resume`, `agent save` / `agent resume`.
//!
//! A checkpoint holds what each conversation is called by its harness —
//! Claude Code's session id, Codex's thread id — and what the member was.
//! Resuming types a command line with that id into a shell, so a file
//! that holds anything but a plain id is refused, as a restored pane's is
//! (`agents::restore_command`): these files are not trusted to hold only
//! what was written.
//!
//! The resume line is the member's launch line — model, effort, limits,
//! and its role's prompt — with the resume in front. The prompt must be
//! passed again: both harnesses keep the one the conversation started
//! with only until it is compacted, then build it from the launch. Claude
//! Code's `--help` says so of `--system-prompt-snapshot` (on by default);
//! Codex writes `developer_instructions` into the rollout as a message
//! and its compaction rebuilds that context from the session's config.

use std::path::{Path, PathBuf};

use cmux_protocol::{Checkpoint, CheckpointMember, Harness};

use crate::agents::is_safe_id;

pub fn saves_dir() -> PathBuf {
    crate::session::data_dir().join("saves")
}

fn path_in(dir: &Path, name: &str) -> Result<PathBuf, String> {
    if !crate::roles::is_name(name) {
        return Err(format!("`{name}` cannot name a save: letters, digits, `-`, `_` and `.` only"));
    }
    Ok(dir.join(format!("{name}.json")))
}

/// Writes `checkpoint` in `dir` under its name, over any save of that name.
pub fn save(dir: &Path, checkpoint: &Checkpoint) -> Result<(), String> {
    let path = path_in(dir, &checkpoint.name)?;
    check(checkpoint)?;
    let json = serde_json::to_vec_pretty(checkpoint).map_err(|e| e.to_string())?;
    crate::session::atomic_write(&path, &json).map_err(|e| format!("could not write the save: {e}"))
}

/// The save `name` in `dir`.
pub fn load(dir: &Path, name: &str) -> Result<Checkpoint, String> {
    let path = path_in(dir, name)?;
    let text = std::fs::read_to_string(&path).map_err(|_| format!("no save `{name}`"))?;
    let checkpoint: Checkpoint = serde_json::from_str(&text).map_err(|e| format!("save `{name}` cannot be read: {e}"))?;
    if checkpoint.name != name {
        return Err(format!("save `{name}` names itself `{}`", checkpoint.name));
    }
    check(&checkpoint).map_err(|e| format!("save `{name}`: {e}"))?;
    Ok(checkpoint)
}

/// Every save in `dir` that loads, newest first.
pub fn list(dir: &Path) -> Vec<Checkpoint> {
    let mut saves: Vec<Checkpoint> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.strip_suffix(".json")?.to_string();
            load(dir, &name).ok()
        })
        .collect();
    saves.sort_by(|a, b| b.saved_at.cmp(&a.saved_at).then_with(|| a.name.cmp(&b.name)));
    saves
}

/// What must hold of a checkpoint before any of it is typed into a shell.
fn check(checkpoint: &Checkpoint) -> Result<(), String> {
    if checkpoint.members.is_empty() {
        return Err("it has no members".into());
    }
    for m in &checkpoint.members {
        if !is_safe_id(&m.session) {
            return Err(format!("member `{}` has a session id that is not a plain id", m.name));
        }
        if let Some(role) = m.role.as_deref().filter(|r| !crate::roles::is_name(r)) {
            return Err(format!("member `{}` has a role `{role}` no role could be named", m.name));
        }
        resumable(m.harness)?;
    }
    Ok(())
}

fn resumable(harness: Harness) -> Result<(), String> {
    match harness {
        Harness::Claude | Harness::Codex => Ok(()),
        other => Err(format!("{} cannot be resumed by Mirador", other.label())),
    }
}

/// The session id of a Claude Code agent, from its pane's `agent_session`.
/// A daemon job (`claude --bg`) is refused: the daemon does not survive
/// the computer being turned off, and its job with it.
pub fn claude_session(agent_session: Option<&str>) -> Result<String, String> {
    let record = agent_session.ok_or("no Claude Code session is known for it yet (are Mirador's hooks set up?)")?;
    match record.split_once(':') {
        Some(("claude-fg", id)) if is_safe_id(id) => Ok(id.to_string()),
        Some(("claude-bg", _)) => {
            Err("it runs in Claude Code's background daemon (claude --bg), whose jobs do not survive a restart".into())
        }
        _ => Err("its Claude Code session is not one Mirador can resume".into()),
    }
}

/// The thread id of the Codex at `pid`, from the rollout it holds open.
/// Windows cannot list another process's open files, so it is refused
/// there.
pub fn codex_session(pid: u32) -> Result<String, String> {
    if cfg!(not(unix)) {
        return Err("Mirador cannot find a Codex conversation on Windows yet".into());
    }
    let rollout = crate::codex_trail::open_rollout(pid).map_err(|n| match n {
        0 => "its Codex holds no conversation open".to_string(),
        n => format!("its Codex holds {n} conversations open, and Mirador cannot tell which is its own"),
    })?;
    crate::codex_trail::rollout_id(&rollout).ok_or_else(|| "its Codex conversation has no id Mirador can read".into())
}

/// What to type to resume `member`, given `launch`: the line that starts
/// it fresh, with its role's flags and no task (`agents::launch_command`).
pub fn resume_command(member: &CheckpointMember, launch: &str) -> Result<String, String> {
    if !is_safe_id(&member.session) {
        return Err(format!("member `{}` has a session id that is not a plain id", member.name));
    }
    let (program, resume) = match member.harness {
        Harness::Claude => ("claude", "--resume"),
        Harness::Codex => ("codex", "resume"),
        other => return Err(format!("{} cannot be resumed by Mirador", other.label())),
    };
    let flags = launch
        .strip_prefix(program)
        .filter(|rest| rest.is_empty() || rest.starts_with(' '))
        .ok_or_else(|| format!("member `{}` would not start {program}", member.name))?;
    Ok(format!("{program} {resume} {}{flags}", member.session))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str, harness: Harness, session: &str) -> CheckpointMember {
        CheckpointMember {
            name: name.into(),
            role: Some("reviewer".into()),
            harness,
            session: session.into(),
            ..Default::default()
        }
    }

    fn checkpoint(name: &str, saved_at: u64) -> Checkpoint {
        Checkpoint {
            name: name.into(),
            saved_at,
            team: Some("squad".into()),
            members: vec![member("planner", Harness::Claude, "abc-1")],
            ..Default::default()
        }
    }

    fn dir(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mirador-saves-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn saves_load_and_list_newest_first() {
        let dir = dir("list");
        save(&dir, &checkpoint("monday", 1)).unwrap();
        save(&dir, &checkpoint("tuesday", 2)).unwrap();
        let mut again = checkpoint("monday", 3);
        again.cwd = Some("/work".into());
        save(&dir, &again).unwrap();
        assert_eq!(load(&dir, "monday"), Ok(again));
        let names: Vec<String> = list(&dir).into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["monday", "tuesday"], "overwritten, and newest first");
        assert!(load(&dir, "friday").unwrap_err().contains("no save `friday`"));
        // A file that does not load is left out of the list, not fatal.
        std::fs::write(dir.join("broken.json"), "{").unwrap();
        assert_eq!(list(&dir).len(), 2);
        assert_eq!(list(&dir.join("none")), Vec::new());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_a_name_or_an_id_a_shell_would_read_twice() {
        let dir = dir("refuse");
        for name in ["", "../up", "a b", "-flag", ".hidden"] {
            assert!(save(&dir, &checkpoint(name, 1)).is_err(), "{name:?}");
            assert!(load(&dir, name).is_err(), "{name:?}");
        }
        let mut evil = checkpoint("evil", 1);
        evil.members[0].session = "abc; rm -rf ~".into();
        assert!(save(&dir, &evil).is_err());
        // Written by something else: load refuses it all the same.
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("evil.json"), serde_json::to_vec(&evil).unwrap()).unwrap();
        assert!(load(&dir, "evil").unwrap_err().contains("not a plain id"));
        let mut role = checkpoint("role", 1);
        role.members[0].role = Some("$(id)".into());
        std::fs::write(dir.join("role.json"), serde_json::to_vec(&role).unwrap()).unwrap();
        assert!(load(&dir, "role").is_err());
        // A file renamed is not the save it says it is.
        std::fs::write(dir.join("other.json"), serde_json::to_vec(&checkpoint("monday", 1)).unwrap()).unwrap();
        assert!(load(&dir, "other").unwrap_err().contains("names itself"));
        assert!(save(&dir, &Checkpoint { members: vec![], ..checkpoint("empty", 1) }).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_claude_session_resumes_only_in_the_pane() {
        assert_eq!(claude_session(Some("claude-fg:abc-1")), Ok("abc-1".into()));
        assert!(claude_session(Some("claude-bg:c5b63b09")).unwrap_err().contains("do not survive a restart"));
        assert!(claude_session(Some("claude:abc-1")).is_err(), "an older version's record");
        assert!(claude_session(Some("claude-fg:$(id)")).is_err());
        assert!(claude_session(None).unwrap_err().contains("hooks"));
    }

    #[test]
    fn resumes_with_the_launch_flags() {
        let claude = member("reviewer-1", Harness::Claude, "abc-1");
        assert_eq!(
            resume_command(&claude, "claude --model 'opus' --append-system-prompt-file '/p/r.md' --name 'reviewer-1'"),
            Ok("claude --resume abc-1 --model 'opus' --append-system-prompt-file '/p/r.md' --name 'reviewer-1'".into())
        );
        assert_eq!(resume_command(&claude, "claude"), Ok("claude --resume abc-1".into()));
        let codex = member("coder", Harness::Codex, "01a16910-50d2-7590-8de0-5e5e7b78b19c");
        assert_eq!(
            resume_command(&codex, "codex --model 'gpt-6-luna' -c 'developer_instructions=\"You code.\"'"),
            Ok("codex resume 01a16910-50d2-7590-8de0-5e5e7b78b19c --model 'gpt-6-luna' -c 'developer_instructions=\"You code.\"'".into())
        );
        // A prompt read from a file by the shell is passed again the same way.
        assert_eq!(
            resume_command(&codex, r#"codex --model 'gpt-6-luna' -c "$(command cat '/p/coder-1.toml')""#),
            Ok(r#"codex resume 01a16910-50d2-7590-8de0-5e5e7b78b19c --model 'gpt-6-luna' -c "$(command cat '/p/coder-1.toml')""#.into())
        );
        assert!(resume_command(&codex, "claude --model 'opus'").is_err(), "not the harness it saved");
        assert!(resume_command(&claude, "claudette").is_err());
        assert!(resume_command(&member("x", Harness::Aider, "a1"), "aider").unwrap_err().contains("Aider"));
        assert!(resume_command(&member("x", Harness::Claude, "a b"), "claude").is_err());
    }
}
