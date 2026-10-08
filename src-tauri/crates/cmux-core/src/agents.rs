//! Which Claude Code session belongs to which pane, and how a pane brings
//! it back after a restart.
//!
//! A hook reports its session with the `MIRA_PANE` it inherited, but that
//! variable is only as good as the process tree it came down. Claude Code
//! can host sessions in a background daemon (`claude --bg`, `claude
//! attach`); the daemon inherits `MIRA_PANE` from whichever pane first
//! started it and hands that to every session it ever runs, so with three
//! tabs attached to three daemon sessions all three claimed the first tab.
//! The terminal a hook runs on cannot be inherited that way, so on unix it
//! is what decides: the pane that owns the hook's tty, or — for a daemon
//! session, whose tty is the daemon's own — the pane whose tty runs
//! `claude attach <job>`.

use std::collections::HashMap;

use cmux_protocol::{AgentRole, AgentStatus};

/// One row of the process table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    /// Without `/dev/`; `None` for a process with no controlling terminal.
    pub tty: Option<String>,
    pub args: String,
}

/// What a `claude` process in a pane is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeProc {
    /// An interactive session running in the pane itself.
    Foreground,
    /// `claude attach <job>`: a view onto a session the daemon hosts.
    Attach(String),
}

/// The process table, on unix. Windows has no ttys to match on and keeps
/// trusting `MIRA_PANE`, so it gets an empty table.
pub fn process_table() -> Vec<Proc> {
    #[cfg(unix)]
    {
        let output = crate::proc::command("ps")
            .args(["-axww", "-o", "pid=,tty=,args="])
            .output();
        match output {
            Ok(o) => parse_ps(&String::from_utf8_lossy(&o.stdout)),
            Err(_) => Vec::new(),
        }
    }
    #[cfg(not(unix))]
    {
        Vec::new()
    }
}

/// Parses `ps -o pid=,tty=,args=`. A tty of `??` (macOS) or `?` (Linux)
/// means none.
pub fn parse_ps(text: &str) -> Vec<Proc> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let (pid, rest) = line.split_once(char::is_whitespace)?;
            let rest = rest.trim_start();
            let (tty, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            Some(Proc {
                pid: pid.parse().ok()?,
                tty: (!tty.contains('?')).then(|| tty.trim_start_matches("/dev/").to_string()),
                args: args.trim().to_string(),
            })
        })
        .collect()
}

/// Classifies a command line as a Claude Code session, if it is one. The
/// daemon's own processes (`claude daemon`, `claude bg-*`) are not.
pub fn classify(args: &str) -> Option<ClaudeProc> {
    let mut words = args.split_whitespace();
    let mut program = words.next()?;
    // An npm install runs as `node …/claude`.
    if basename(program) == "node" {
        program = words.next()?;
    }
    if basename(program) != "claude" {
        return None;
    }
    match words.next() {
        Some("attach") => words
            .next()
            .filter(|job| is_safe_id(job))
            .map(|job| ClaudeProc::Attach(job.to_string())),
        Some(sub) if sub == "daemon" || sub.starts_with("bg-") => None,
        _ => Some(ClaudeProc::Foreground),
    }
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Pane → tty, from each pane's shell pid.
pub fn pane_ttys(procs: &[Proc], pane_pids: &[(String, u32)]) -> HashMap<String, String> {
    let tty_of: HashMap<u32, &str> = procs
        .iter()
        .filter_map(|p| Some((p.pid, p.tty.as_deref()?)))
        .collect();
    pane_pids
        .iter()
        .filter_map(|(pane, pid)| Some((pane.clone(), tty_of.get(pid)?.to_string())))
        .collect()
}

/// The pane a hook belongs to. `claimed` is the inherited `MIRA_PANE`
/// (already checked to name a live pane), `tty` the hook's terminal, and
/// `job` the daemon job id when the session runs in the background.
///
/// When a tty or job is known it decides, and finding no pane for it means
/// the hook belongs to none: the claim was inherited from somewhere else.
/// Only without either (Windows) does the claim stand.
pub fn hook_pane(
    procs: &[Proc],
    pane_pids: &[(String, u32)],
    claimed: Option<String>,
    tty: Option<&str>,
    job: Option<&str>,
) -> Option<String> {
    let ttys = pane_ttys(procs, pane_pids);
    let pane_on = |wanted: &str| {
        let wanted = wanted.trim_start_matches("/dev/");
        let mut panes: Vec<&String> = ttys
            .iter()
            .filter(|(_, t)| t.as_str() == wanted)
            .map(|(p, _)| p)
            .collect();
        panes.sort();
        panes.first().map(|p| p.to_string())
    };
    if let Some(job) = job {
        // Several panes may attach the same job; the lowest pid (the
        // longest-attached) wins so the answer is stable between hooks.
        let mut attaching: Vec<&Proc> = procs
            .iter()
            .filter(|p| classify(&p.args) == Some(ClaudeProc::Attach(job.to_string())))
            .collect();
        attaching.sort_by_key(|p| p.pid);
        return attaching
            .iter()
            .find_map(|p| p.tty.as_deref().and_then(pane_on));
    }
    if let Some(tty) = tty {
        return pane_on(tty);
    }
    claimed
}

/// Panes whose terminal is running a Claude Code session right now, each
/// with that process's command line (the oldest, when there are several).
pub fn panes_running_claude(procs: &[Proc], pane_pids: &[(String, u32)]) -> HashMap<String, String> {
    let mut by_tty: HashMap<&str, &Proc> = HashMap::new();
    for p in procs.iter().filter(|p| classify(&p.args).is_some()) {
        let Some(tty) = p.tty.as_deref() else { continue };
        let slot = by_tty.entry(tty).or_insert(p);
        if p.pid < slot.pid {
            *slot = p;
        }
    }
    pane_ttys(procs, pane_pids)
        .into_iter()
        .filter_map(|(pane, tty)| Some((pane, by_tty.get(tty.as_str())?.args.clone())))
        .collect()
}

/// The `--model` a Claude Code command line asks for.
pub fn model_flag(args: &str) -> Option<String> {
    let mut words = args.split_whitespace();
    while let Some(word) = words.next() {
        if word == "--model" {
            return words.next().map(String::from);
        }
        if let Some(model) = word.strip_prefix("--model=") {
            return Some(model.to_string());
        }
    }
    None
}

/// What a `mira claude-hook` event says the agent is doing now; `None`
/// leaves it as it was.
///
/// A Notification is either Claude blocked on the human (a permission
/// prompt, a question) or the reminder that it has been idle a while, which
/// changes nothing — it already stopped. Claude Code names the kind in
/// `notification_type`; without it the message is all there is.
pub fn hook_status(
    event: &str,
    notification_type: Option<&str>,
    message: Option<&str>,
) -> Option<AgentStatus> {
    match event {
        "prompt-submit" | "post-tool" => Some(AgentStatus::Working),
        "stop" | "stop-failure" | "session-start" => Some(AgentStatus::Idle),
        "notification" => match notification_type {
            Some("permission_prompt" | "elicitation_dialog") => Some(AgentStatus::NeedsYou),
            Some(_) => None,
            None if message.is_some_and(|m| m.contains("waiting for your input")) => None,
            None => Some(AgentStatus::NeedsYou),
        },
        _ => None,
    }
}

/// The command line that starts an agent in a shell pane: `claude` with
/// the role's model and prompt, named after the role, and the task as its
/// first message. `quote` quotes one argument for the pane's shell.
///
/// It is typed in, so every argument is put on one line and loses its
/// control characters: a newline would submit the line half-quoted, and a
/// stray `^C` would be read as a keystroke before the shell ever saw quotes.
pub fn launch_command(
    role: Option<&AgentRole>,
    task: Option<&str>,
    quote: impl Fn(&str) -> String,
) -> String {
    let mut line = String::from("claude");
    let mut arg = |flag: &str, value: &str| {
        let value = one_line(value);
        if value.is_empty() {
            return;
        }
        if !flag.is_empty() {
            line.push(' ');
            line.push_str(flag);
        }
        line.push(' ');
        line.push_str(&quote(&value));
    };
    if let Some(role) = role {
        arg("--model", role.model.as_deref().unwrap_or(""));
        arg("--append-system-prompt", role.prompt.as_deref().unwrap_or(""));
        arg("--name", &role.name);
    }
    arg("", task.unwrap_or(""));
    line
}

fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `agent_session` value recorded for a hook: `claude-fg:<session>` for
/// a session in the pane, `claude-bg:<job>` for one the daemon hosts.
///
/// Not `claude:<session>`: that is what versions before this wrote, using
/// the inherited pane id, so a daemon session could be filed under any pane
/// as a plain session id. Auto-resuming one of those could start a second
/// process on a conversation the daemon is still running — see
/// [`legacy_resume_command`].
pub fn session_record(session_id: &str, job: Option<&str>) -> Option<String> {
    match job {
        Some(job) if is_safe_id(job) => Some(format!("claude-bg:{job}")),
        Some(_) => None,
        None if is_safe_id(session_id) => Some(format!("claude-fg:{session_id}")),
        None => None,
    }
}

/// What to type into a restored pane to bring its session back. The id is
/// typed into a shell, so anything but a plain id is refused rather than
/// quoted — the session file is not trusted to hold only what we wrote.
///
/// `model` is a role's `--model`, already quoted for the shell: a resumed
/// conversation keeps its system prompt but not its model, so a role's
/// pane would otherwise come back on the default one. A daemon session
/// keeps running on whatever it had, and attaching takes no model.
pub fn restore_command(agent_session: &str, model: Option<&str>) -> Option<String> {
    let (agent, id) = agent_session.split_once(':')?;
    if !is_safe_id(id) {
        return None;
    }
    match agent {
        "claude-fg" => Some(match model {
            Some(model) => format!("claude --resume {id} --model {model}"),
            None => format!("claude --resume {id}"),
        }),
        "claude-bg" => Some(format!("claude attach {id}")),
        _ => None,
    }
}

/// A `claude:<session>` record from an older version. It may have been
/// misattributed, so it is never typed in on its own: the pane comes back
/// idle offering this command on a keypress, as it always did, and the
/// next hook replaces the record with one that can be trusted.
pub fn legacy_resume_command(agent_session: &str) -> Option<String> {
    let id = agent_session.strip_prefix("claude:")?;
    is_safe_id(id).then(|| format!("claude --resume {id}"))
}

/// Whether a restored Claude pane may resume by itself. Only where the
/// poller can see Claude exit (it reads the process table) is an exited
/// session forgotten; without that, auto-resume would bring back on every
/// launch a conversation you closed long ago. Windows lists no processes
/// here, so its panes wait for a keypress as they always did.
pub const AUTO_RESUME: bool = cfg!(unix);

/// How a pane comes back from the session file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Restored {
    /// Command pane: runs this, idle until a keypress.
    pub command: Option<String>,
    /// Shell pane: types this in once the shell first prints.
    pub startup_input: Option<String>,
}

/// Decides how a saved pane is restored, given its saved `command`, whether
/// it is remote, its `agent_session`, its role's quoted `model` (see
/// [`restore_command`]), and [`AUTO_RESUME`] (a parameter so both
/// platforms' behavior is tested everywhere).
pub fn restore_pane(
    command: Option<&str>,
    remote: bool,
    agent_session: Option<&str>,
    model: Option<&str>,
    auto_resume: bool,
) -> Restored {
    // A resume this app put there, saved back as a command pane — by an
    // older version, or by a platform without auto-resume. It is derived
    // again from `agent_session` below, or the pane would come back as a
    // command pane on every launch after the session is gone.
    let command = command
        .filter(|c| !c.starts_with("claude --resume ") && !c.starts_with("claude attach "));
    let keep = Restored {
        command: command.map(str::to_string),
        startup_input: None,
    };
    // A real command pane (`mira run claude …`) or an SSH pane keeps its own.
    let Some(session) = agent_session.filter(|_| command.is_none() && !remote) else {
        return keep;
    };
    if let Some(input) = restore_command(session, model) {
        if auto_resume {
            Restored { command: None, startup_input: Some(input) }
        } else {
            Restored { command: Some(input), startup_input: None }
        }
    } else {
        // An older version's record, possibly misattributed: offered on a
        // keypress like any restored command pane, never typed in.
        Restored {
            command: legacy_resume_command(session),
            startup_input: None,
        }
    }
}

/// The job id from a `CLAUDE_JOB_DIR` (`…/jobs/<job>`).
pub fn job_from_dir(dir: &str) -> Option<String> {
    let job = basename(dir.trim_end_matches(['/', '\\']));
    is_safe_id(job).then(|| job.to_string())
}

fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The process table that showed the bug: a daemon started from pane A,
    /// whose sessions all carry A's `MIRA_PANE`; pane A attaches job
    /// c5b63b09, pane B attaches 1234abcd, pane C runs claude itself.
    const PS: &str = "\
  101 ttys002  -/bin/zsh
  102 ttys004  -/bin/zsh
  103 ttys006  -/bin/zsh
  200 ??       /Users/u/.local/bin/claude daemon run --origin transient
  201 ttys009  claude bg-spare --bg-spare /tmp/cc/spare.sock
  202 ttys010  claude bg-spare --bg-spare /tmp/cc/other.sock
  300 ttys002  /Users/u/.local/bin/claude attach c5b63b09
  301 ttys004  claude attach 1234abcd
  302 ttys006  claude --resume 228fb346-0d12-4618-824c-a0bc432bc2ab
  400 ttys009  /bin/sh -c mira claude-hook stop
";

    fn panes() -> Vec<(String, u32)> {
        vec![("A".into(), 101), ("B".into(), 102), ("C".into(), 103)]
    }

    #[test]
    fn parses_ps_rows() {
        let procs = parse_ps(PS);
        assert_eq!(procs.len(), 10);
        assert_eq!(procs[3].tty, None);
        assert_eq!(procs[6].tty.as_deref(), Some("ttys002"));
        assert_eq!(procs[6].args, "/Users/u/.local/bin/claude attach c5b63b09");
    }

    #[test]
    fn classifies_claude_processes() {
        assert_eq!(classify("claude"), Some(ClaudeProc::Foreground));
        assert_eq!(classify("/a/b/claude --resume x"), Some(ClaudeProc::Foreground));
        assert_eq!(
            classify("node /usr/local/bin/claude attach ab12"),
            Some(ClaudeProc::Attach("ab12".into()))
        );
        assert_eq!(classify("claude daemon run"), None);
        assert_eq!(classify("claude bg-spare --bg-spare x"), None);
        assert_eq!(classify("claude attach ;rm"), None);
        assert_eq!(classify("-/bin/zsh"), None);
        assert_eq!(classify("vim claude"), None);
    }

    #[test]
    fn daemon_hook_lands_on_the_attaching_pane_not_the_inherited_one() {
        let procs = parse_ps(PS);
        // A daemon session for job 1234abcd still claims pane A, and runs on
        // the daemon's tty, which no pane owns.
        let pane = hook_pane(&procs, &panes(), Some("A".into()), Some("ttys009"), Some("1234abcd"));
        assert_eq!(pane.as_deref(), Some("B"));
    }

    #[test]
    fn unattached_daemon_session_belongs_to_no_pane() {
        let procs = parse_ps(PS);
        let pane = hook_pane(&procs, &panes(), Some("A".into()), Some("ttys009"), Some("ffff0000"));
        assert_eq!(pane, None);
    }

    #[test]
    fn foreground_hook_uses_its_tty_over_the_claim() {
        let procs = parse_ps(PS);
        let pane = hook_pane(&procs, &panes(), Some("A".into()), Some("/dev/ttys006"), None);
        assert_eq!(pane.as_deref(), Some("C"));
    }

    #[test]
    fn a_tty_no_pane_owns_rejects_the_claim() {
        let procs = parse_ps(PS);
        let pane = hook_pane(&procs, &panes(), Some("A".into()), Some("ttys010"), None);
        assert_eq!(pane, None);
    }

    #[test]
    fn without_tty_or_job_the_claim_stands() {
        // Windows: no ttys, an empty process table.
        let pane = hook_pane(&[], &panes(), Some("B".into()), None, None);
        assert_eq!(pane.as_deref(), Some("B"));
    }

    #[test]
    fn finds_panes_running_claude() {
        let procs = parse_ps(PS);
        let mut panes4 = panes();
        panes4.push(("D".into(), 999)); // a pane whose shell is gone
        let running = panes_running_claude(&procs, &panes4);
        let mut found: Vec<&str> = running.keys().map(String::as_str).collect();
        found.sort();
        assert_eq!(found, ["A", "B", "C"]);
        assert_eq!(running["C"], "claude --resume 228fb346-0d12-4618-824c-a0bc432bc2ab");

        let idle = parse_ps("  101 ttys002  -/bin/zsh\n  200 ??  claude daemon run\n");
        assert!(panes_running_claude(&idle, &panes()).is_empty());
    }

    #[test]
    fn records_and_restores_sessions() {
        assert_eq!(session_record("abc-1", None).as_deref(), Some("claude-fg:abc-1"));
        assert_eq!(session_record("abc-1", Some("c5b63b09")).as_deref(), Some("claude-bg:c5b63b09"));
        assert_eq!(session_record("x y", None), None);

        assert_eq!(restore_command("claude-fg:abc-1", None).as_deref(), Some("claude --resume abc-1"));
        assert_eq!(restore_command("claude-bg:c5b63b09", None).as_deref(), Some("claude attach c5b63b09"));
        assert_eq!(restore_command("claude-fg:abc; rm -rf ~", None), None);
        assert_eq!(restore_command("claude-bg:$(id)", None), None);
        assert_eq!(restore_command("other:abc", None), None);
        assert_eq!(restore_command("claude-fg:", None), None);
        // Older records are never auto-typed, only offered on a keypress.
        assert_eq!(restore_command("claude:abc-1", None), None);
        // A role's model comes back with its conversation; an attach has none.
        assert_eq!(
            restore_command("claude-fg:abc-1", Some("'opus'")).as_deref(),
            Some("claude --resume abc-1 --model 'opus'")
        );
        assert_eq!(
            restore_command("claude-bg:c5b63b09", Some("'opus'")).as_deref(),
            Some("claude attach c5b63b09")
        );
        assert_eq!(legacy_resume_command("claude:abc-1").as_deref(), Some("claude --resume abc-1"));
        assert_eq!(legacy_resume_command("claude:a;b"), None);
        assert_eq!(legacy_resume_command("claude-fg:abc-1"), None);
    }

    #[test]
    fn restores_claude_panes_by_platform() {
        let typed = |s: &str| Restored { command: None, startup_input: Some(s.into()) };
        let idle = |s: &str| Restored { command: Some(s.into()), startup_input: None };

        // unix: typed into the shell.
        assert_eq!(restore_pane(None, false, Some("claude-fg:a1"), None, true), typed("claude --resume a1"));
        assert_eq!(restore_pane(None, false, Some("claude-bg:j9"), None, true), typed("claude attach j9"));
        // Windows: idle, a keypress resumes, as before.
        assert_eq!(restore_pane(None, false, Some("claude-fg:a1"), None, false), idle("claude --resume a1"));
        assert_eq!(restore_pane(None, false, Some("claude-bg:j9"), None, false), idle("claude attach j9"));
        // The command Windows saved back is derived again, not kept: once the
        // session is gone the pane is a plain shell.
        assert_eq!(
            restore_pane(Some("claude attach j9"), false, Some("claude-bg:j9"), None, false),
            idle("claude attach j9")
        );
        assert_eq!(restore_pane(Some("claude attach j9"), false, None, None, false), Restored::default());
        assert_eq!(restore_pane(Some("claude --resume a1"), false, None, None, true), Restored::default());
    }

    #[test]
    fn restore_leaves_other_panes_alone() {
        // Legacy records are offered on a keypress on every platform.
        for auto in [true, false] {
            assert_eq!(
                restore_pane(Some("claude --resume old"), false, Some("claude:old"), None, auto),
                Restored { command: Some("claude --resume old".into()), startup_input: None }
            );
        }
        // A real command pane and an SSH pane keep what they had.
        assert_eq!(
            restore_pane(Some("npm test"), false, Some("claude-fg:a1"), None, true),
            Restored { command: Some("npm test".into()), startup_input: None }
        );
        assert_eq!(restore_pane(None, true, Some("claude-fg:a1"), None, true), Restored::default());
        // A plain shell, and a record that is not ours.
        assert_eq!(restore_pane(None, false, None, None, true), Restored::default());
        assert_eq!(restore_pane(None, false, Some("claude-fg:a;b"), None, true), Restored::default());
    }

    #[test]
    fn job_id_from_its_directory() {
        assert_eq!(job_from_dir("/Users/u/.claude/jobs/c5b63b09").as_deref(), Some("c5b63b09"));
        assert_eq!(job_from_dir("/Users/u/.claude/jobs/c5b63b09/").as_deref(), Some("c5b63b09"));
        assert_eq!(job_from_dir("/tmp/jobs/a b"), None);
        assert_eq!(job_from_dir(""), None);
    }

    #[test]
    fn restores_a_role_pane_on_its_model() {
        assert_eq!(
            restore_pane(None, false, Some("claude-fg:a1"), Some("'opus'"), true),
            Restored { command: None, startup_input: Some("claude --resume a1 --model 'opus'".into()) }
        );
    }

    #[test]
    fn reads_the_model_flag() {
        assert_eq!(model_flag("claude --model opus --resume x").as_deref(), Some("opus"));
        assert_eq!(model_flag("node /a/claude --model=claude-sonnet-5-5").as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(model_flag("claude --resume x"), None);
        assert_eq!(model_flag("claude --model"), None);
    }

    #[test]
    fn hook_events_set_the_status() {
        use AgentStatus::*;
        assert_eq!(hook_status("prompt-submit", None, None), Some(Working));
        assert_eq!(hook_status("post-tool", None, None), Some(Working));
        assert_eq!(hook_status("stop", None, None), Some(Idle));
        // A turn that ended on an API error is over too.
        assert_eq!(hook_status("stop-failure", None, None), Some(Idle));
        assert_eq!(hook_status("session-start", None, None), Some(Idle));
        assert_eq!(hook_status("notification", Some("permission_prompt"), Some("Claude needs your permission to use Bash")), Some(NeedsYou));
        assert_eq!(hook_status("notification", Some("elicitation_dialog"), None), Some(NeedsYou));
        // Idle reminders and auth notices change nothing.
        assert_eq!(hook_status("notification", Some("idle_prompt"), Some("Claude is waiting for your input")), None);
        assert_eq!(hook_status("notification", Some("auth_success"), None), None);
        // Without a type, the message decides.
        assert_eq!(hook_status("notification", None, Some("Claude is waiting for your input")), None);
        assert_eq!(hook_status("notification", None, Some("Claude needs your permission to use Edit")), Some(NeedsYou));
        assert_eq!(hook_status("something-new", None, None), None);
    }

    #[test]
    fn builds_the_launch_command() {
        let q = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
        assert_eq!(launch_command(None, None, q), "claude");
        assert_eq!(launch_command(None, Some("fix the flaky test"), q), "claude 'fix the flaky test'");
        let role = AgentRole {
            name: "reviewer".into(),
            model: Some("opus".into()),
            prompt: Some("You review.\nNever edit files; it's read-only.".into()),
        };
        assert_eq!(
            launch_command(Some(&role), Some("look at\r\nmain\u{3}"), q),
            r"claude --model 'opus' --append-system-prompt 'You review. Never edit files; it'\''s read-only.' --name 'reviewer' 'look at main'"
        );
        let bare = AgentRole { name: "plain".into(), model: None, prompt: None };
        assert_eq!(launch_command(Some(&bare), Some("  "), q), "claude --name 'plain'");
    }
}
