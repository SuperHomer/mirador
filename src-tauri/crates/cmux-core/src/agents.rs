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

use cmux_protocol::{AgentInfo, AgentRole, AgentStatus, Harness};

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

/// The harness a command line runs, with the words after its program.
///
/// The program may sit behind its interpreter: an npm install runs as
/// `node …/codex`, a pip one as `python3 …/aider` or `python -m aider`.
/// Subcommands that serve rather than converse — `codex mcp-server`,
/// `opencode serve`, Claude Code's own daemon — are not agents: they run
/// for something else, and a pane holding one is not working on anything.
pub fn harness_of(args: &str) -> Option<(Harness, Vec<&str>)> {
    let mut words = args.split_whitespace().peekable();
    let mut program = words.next()?;
    let interpreter = program_name(program);
    if matches!(interpreter, "node" | "bun" | "deno") || interpreter.starts_with("python") {
        // Its own flags, then the script — or, for `python -m`, the module.
        loop {
            let word = words.next()?;
            if word == "-m" {
                program = words.next()?;
                break;
            }
            if !word.starts_with('-') {
                program = word;
                break;
            }
        }
    }
    let name = program_name(program);
    let (_, harness) = HARNESSES.iter().find(|(program, _)| *program == name)?;
    let rest: Vec<&str> = words.collect();
    let serving = rest.first().is_some_and(|sub| {
        matches!(
            *sub,
            "daemon" | "mcp" | "mcp-server" | "app-server" | "serve" | "server" | "web" | "acp"
        ) || sub.starts_with("bg-")
    });
    (!serving).then_some((*harness, rest))
}

/// Each harness by the name of the program that runs it.
const HARNESSES: [(&str, Harness); 13] = [
    ("claude", Harness::Claude),
    ("codex", Harness::Codex),
    ("opencode", Harness::OpenCode),
    ("pi", Harness::Pi),
    ("gemini", Harness::Gemini),
    ("amp", Harness::Amp),
    ("cursor-agent", Harness::CursorAgent),
    ("copilot", Harness::Copilot),
    ("aider", Harness::Aider),
    ("goose", Harness::Goose),
    ("qwen", Harness::Qwen),
    ("crush", Harness::Crush),
    ("droid", Harness::Droid),
];

/// A program's name without its directory or extension: `codex` for
/// `…/bin/codex.js`, `claude` for `C:\…\claude.exe`.
fn program_name(path: &str) -> &str {
    let name = basename(path);
    [".exe", ".cmd", ".js", ".cjs", ".mjs"]
        .iter()
        .find_map(|ext| name.strip_suffix(ext))
        .unwrap_or(name)
}

/// Classifies a command line as a Claude Code session, if it is one. The
/// daemon's own processes (`claude daemon`, `claude bg-*`) are not.
pub fn classify(args: &str) -> Option<ClaudeProc> {
    let (Harness::Claude, rest) = harness_of(args)? else {
        return None;
    };
    let mut rest = rest.into_iter();
    match rest.next() {
        Some("attach") => rest
            .next()
            .filter(|job| is_safe_id(job))
            .map(|job| ClaudeProc::Attach(job.to_string())),
        _ => Some(ClaudeProc::Foreground),
    }
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Whether the shell `shell_pid` is running something: another process on
/// its terminal. Typing a command into it then would type into whatever
/// that is — an editor, a dev server — so a team member is only restarted
/// in a pane whose shell is idle. Unknown (no such process, no terminal)
/// is busy: better to refuse than to type blind.
pub fn shell_busy(procs: &[Proc], shell_pid: u32) -> bool {
    let Some(tty) = procs.iter().find(|p| p.pid == shell_pid).and_then(|p| p.tty.as_deref()) else {
        return true;
    };
    procs.iter().any(|p| p.pid != shell_pid && p.tty.as_deref() == Some(tty))
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

/// An agent the process table shows in a pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningAgent {
    pub harness: Harness,
    pub pid: u32,
    /// Its command line.
    pub args: String,
}

/// Panes whose terminal is running an agent right now, each with the
/// oldest such process on it: the one that was typed in, not whatever it
/// started since (Codex's npm wrapper runs a native `codex` beneath it).
pub fn panes_running_agents(
    procs: &[Proc],
    pane_pids: &[(String, u32)],
) -> HashMap<String, RunningAgent> {
    let mut by_tty: HashMap<&str, (&Proc, Harness)> = HashMap::new();
    for p in procs {
        let Some(tty) = p.tty.as_deref() else { continue };
        let Some((harness, _)) = harness_of(&p.args) else { continue };
        let slot = by_tty.entry(tty).or_insert((p, harness));
        if p.pid < slot.0.pid {
            *slot = (p, harness);
        }
    }
    pane_ttys(procs, pane_pids)
        .into_iter()
        .filter_map(|(pane, tty)| {
            let (p, harness) = by_tty.get(tty.as_str())?;
            Some((pane, RunningAgent { harness: *harness, pid: p.pid, args: p.args.clone() }))
        })
        .collect()
}

/// What Claude Code says about one of its running sessions, in
/// `<config>/sessions/<pid>.json` — what `claude agents` lists.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClaudeSession {
    pub pid: u32,
    pub session_id: Option<String>,
    /// A background session's job.
    pub job_id: Option<String>,
    /// Its name: one given with `--name` or `/rename`, or else one Claude
    /// Code made up (from the folder at first, then from the conversation).
    pub name: Option<String>,
    /// `busy` or `idle`, as Claude Code last wrote it.
    pub status: Option<AgentStatus>,
}

/// One session file. Not a documented format: every field is optional,
/// and a name is put on one line, since it is drawn in a single row.
pub fn parse_claude_session(json: &str) -> Option<ClaudeSession> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let text = |key: &str| {
        v[key]
            .as_str()
            .map(one_line)
            .filter(|s| !s.is_empty())
    };
    Some(ClaudeSession {
        pid: u32::try_from(v["pid"].as_u64()?).ok()?,
        session_id: text("sessionId"),
        job_id: text("jobId"),
        name: text("name"),
        status: match v["status"].as_str() {
            Some("busy") => Some(AgentStatus::Working),
            Some("idle") => Some(AgentStatus::Idle),
            _ => None,
        },
    })
}

/// Every session file in `dir`.
pub fn claude_sessions(dir: &std::path::Path) -> Vec<ClaudeSession> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| parse_claude_session(&std::fs::read_to_string(e.path()).ok()?))
        .collect()
}

/// Where Claude Code keeps its session files. The transcript the hooks
/// reported (`<config>/projects/<project>/<session>.jsonl`) says which
/// config directory the session uses, whatever this app's own home is;
/// without one, the default `~/.claude`.
pub fn sessions_dir(transcript: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    let from_transcript = transcript.and_then(|t| {
        let mut up = t.ancestors().skip(2);
        let projects = up.next()?;
        (projects.file_name()? == "projects").then(|| up.next())?
    });
    match from_transcript {
        Some(config) => Some(config.join("sessions")),
        None => crate::claude_integration::claude_dir().map(|d| d.join("sessions")),
    }
}

/// The session a pane's Claude Code is: the one its process is (`pid`),
/// else the one its hooks recorded (`agent_session`), else the background
/// job it attaches (`args`, its command line).
pub fn find_claude_session<'a>(
    sessions: &'a [ClaudeSession],
    pid: Option<u32>,
    agent_session: Option<&str>,
    args: Option<&str>,
) -> Option<&'a ClaudeSession> {
    if let Some(found) = pid.and_then(|pid| sessions.iter().find(|s| s.pid == pid)) {
        return Some(found);
    }
    let attached = match args.and_then(classify) {
        Some(ClaudeProc::Attach(job)) => Some(job),
        _ => None,
    };
    let (kind, id) = agent_session.and_then(|s| s.split_once(':')).unzip();
    let job = attached.as_deref().or(id.filter(|_| kind == Some("claude-bg")));
    let session = id.filter(|_| kind == Some("claude-fg"));
    sessions.iter().find(|s| {
        (job.is_some() && s.job_id.as_deref() == job)
            || (session.is_some() && s.session_id.as_deref() == session)
    })
}

/// How long an agent pane may go without its `claude` appearing in the
/// process table before it stops counting as an agent.
pub const START_GRACE: std::time::Duration = std::time::Duration::from_secs(20);

/// Whether the poller should forget a pane's agent: everything that makes
/// it one (session, role, the launch mark) and its status.
///
/// `running`: its `claude` is in the process table now. `seen`: it has been
/// this run. `typing`: a resume is still to be typed in. `waited`: how long
/// since it was started or resumed, when it was.
///
/// One that ran and stopped was exited. One that never showed up in the
/// grace period was never going to: a role pane restored with no
/// conversation to resume (nothing types `claude`), or a resume Claude
/// refused ("No conversation found") and exited before the poller looked.
/// Without this, such panes stayed agents for good — listed under Agents,
/// counted on their tab, with no Claude anywhere.
pub fn forget_agent(
    running: bool,
    seen: bool,
    typing: bool,
    waited: Option<std::time::Duration>,
) -> bool {
    if running || typing {
        return false;
    }
    seen || waited.is_some_and(|w| w >= START_GRACE)
}

/// What an agent's own terminal title says it is working on, for its tile:
/// Claude Code titles its terminal with its session's name or a summary of
/// the task, behind a status glyph it animates (`✳`, `✶`, `⠂`, …). The
/// glyph is dropped, and so is a title that says nothing the tile does not
/// already — empty, the harness's own name or program, or the agent's role
/// or session name (`shown`).
pub fn title_topic(title: &str, harness: Harness, shown: &[Option<&str>]) -> Option<String> {
    let topic = title
        .trim()
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .trim();
    let same = |other: &str| topic.eq_ignore_ascii_case(other);
    let program = HARNESSES.iter().find(|(_, h)| *h == harness).map(|(p, _)| *p);
    if topic.is_empty()
        || same(harness.label())
        || program.is_some_and(same)
        || shown.iter().flatten().any(|s| same(s))
    {
        return None;
    }
    Some(topic.to_string())
}

/// What an agent is called in the sidebar and on the wall — its team
/// name, its session's name, its role, else its harness — as `agentLabel`
/// in `bindings.ts` has it.
pub fn label(agent: &AgentInfo) -> &str {
    agent
        .member
        .as_deref()
        .or(agent.name.as_deref())
        .or(agent.role.as_deref())
        .unwrap_or(agent.harness.label())
}

/// The title a notification from a pane is shown with. An agent's names
/// the agent and its tab (`reviewer · api`): Claude Code titles every one
/// of its alerts just "Claude Code", which says nothing about which of
/// several agents wants you. Any other pane keeps the title it sent.
pub fn notification_title(agent: Option<&AgentInfo>, tab: &str, sent: Option<String>) -> Option<String> {
    match agent {
        Some(agent) => Some(format!("{} · {tab}", label(agent))),
        None => sent,
    }
}

/// The model an agent's command line asks for: `--model x`,
/// `--model=x`, or `-m x` (Codex, Gemini CLI, OpenCode). Only the words
/// after the program are read, so `python -m aider` names no model.
pub fn model_flag(args: &str) -> Option<String> {
    let (_, rest) = harness_of(args)?;
    let mut words = rest.into_iter();
    while let Some(word) = words.next() {
        if word == "--model" || word == "-m" {
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

/// The harness an agent started without a role runs: Claude Code unless
/// asked for another, which must be one Mirador knows how to start plain —
/// Codex. A role names its own.
pub fn plain_harness(harness: Option<Harness>) -> Result<Harness, String> {
    match harness.unwrap_or(Harness::Claude) {
        h @ (Harness::Claude | Harness::Codex) => Ok(h),
        h => Err(format!("an agent without a role is Claude Code or Codex, not {}", h.label())),
    }
}

/// The command line that starts an agent in a shell pane: Claude Code, or
/// the role's harness, with the role's model and prompt, and the task as
/// its first message; without a role, `plain` ([`plain_harness`]). `quote`
/// quotes one argument for the pane's shell.
///
/// The prompt is the role's whole prompt (its own, then its way of
/// working's). Claude Code reads it from `prompt_file` when there is one —
/// a prompt is paragraphs, and a file keeps them — and Codex takes it as
/// its `developer_instructions`, a TOML string whose newlines are escapes:
/// from `prompt_file`, which then holds the whole override
/// ([`codex_instructions`]) and is read with `$(…)` — so only for a shell
/// that takes it (`pty::shell::takes_command_substitution`) — or inline
/// without one.
/// `settings_file` holds a Claude Code role to its MCP servers, plugins,
/// skills and tools (`role_limits`).
///
/// `socket` is Mirador's own, with the permissions profile Codex would
/// otherwise run on ([`codex_base_profile`]): Codex's sandbox refuses every
/// Unix socket, so without it the `mira` a Codex agent runs cannot reach
/// the app. Codex is given that same profile plus the socket and nothing
/// else — its network goes through Codex's proxy, which lets out only what
/// the profile already allowed.
///
/// It is typed in, so every argument is put on one line and loses its
/// control characters: a newline would submit the line half-quoted, and a
/// stray `^C` would be read as a keystroke before the shell ever saw quotes.
#[allow(clippy::too_many_arguments)]
pub fn launch_command(
    role: Option<&AgentRole>,
    plain: Harness,
    prompt: Option<&str>,
    prompt_file: Option<&std::path::Path>,
    settings_file: Option<&std::path::Path>,
    task: Option<&str>,
    socket: Option<(&str, &std::path::Path)>,
    quote: impl Fn(&str) -> String,
) -> String {
    let harness = role.map_or(plain, |r| r.harness);
    let mut line = String::from(match harness {
        Harness::Codex => "codex",
        _ => "claude",
    });
    let arg = |line: &mut String, flag: &str, value: &str| {
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
    let prompt = prompt.map(str::trim).filter(|p| !p.is_empty());
    if let Some(role) = role {
        arg(&mut line, "--model", role.model.as_deref().unwrap_or(""));
        if let Some(effort) = &role.effort {
            match harness {
                Harness::Codex => arg(&mut line, "-c", &format!("model_reasoning_effort={}", toml_string(effort))),
                _ => arg(&mut line, "--effort", effort),
            }
        }
        match harness {
            Harness::Codex => {
                arg(&mut line, "--profile", role.profile.as_deref().unwrap_or(""));
                match (prompt, prompt_file) {
                    // Read by the shell as one word: what is typed stays
                    // short whatever the prompt's length (`codex_instructions`).
                    // `command`, so a `cat` aliased to something else is
                    // not the one run.
                    (Some(_), Some(file)) => {
                        line.push_str(&format!(" -c \"$(command cat {})\"", quote(&file.to_string_lossy())))
                    }
                    (Some(prompt), None) => arg(&mut line, "-c", &codex_instructions(prompt)),
                    (None, _) => {}
                }
            }
            _ => {
                match (prompt, prompt_file) {
                    (Some(_), Some(file)) => arg(&mut line, "--append-system-prompt-file", &file.to_string_lossy()),
                    (Some(prompt), None) => arg(&mut line, "--append-system-prompt", prompt),
                    (None, _) => {}
                }
                if let Some(file) = settings_file {
                    arg(&mut line, "--settings", &file.to_string_lossy());
                }
                arg(&mut line, "--name", &role.name);
            }
        }
    }
    // Any Codex, a role's or plain, may reach the app.
    if let (Harness::Codex, Some((base, socket))) = (harness, socket) {
        arg(&mut line, "-c", "default_permissions=\"mirador\"");
        arg(&mut line, "-c", &format!("permissions.mirador.extends={}", toml_string(base)));
        let socket = toml_string(&socket.to_string_lossy());
        arg(&mut line, "-c", &format!("permissions.mirador.network.unix_sockets={{{socket}=\"allow\"}}"));
        // Network on, but only through the proxy: it lets out the domains
        // the base allows, and the socket above.
        arg(&mut line, "-c", "permissions.mirador.network.enabled=true");
        arg(&mut line, "-c", "features.network_proxy=true");
    }
    arg(&mut line, "", task.unwrap_or(""));
    line
}

/// The permissions profile a Codex started in `cwd` runs on, read from the
/// person's Codex `config` (its text; `None` when there is no file): the
/// base Mirador's own profile extends, so allowing its socket loosens
/// nothing else. `None` means add nothing — a config that does not parse,
/// one on the older `sandbox_mode` (Codex then ignores profiles), or full
/// access (which no profile can extend, and which reaches the socket
/// anyway). With no profile named, it is Codex's own default: the
/// workspace one for a trusted project — `cwd`, or the repository it is
/// in — and read-only for any other, or when `cwd` is not known.
///
/// `profile` is the text of the role's `--profile`, `<name>.config.toml`
/// beside config.toml: what it says wins over config.toml, as it does in
/// Codex. One that does not parse is not guessed at either.
pub fn codex_base_profile(config: Option<&str>, cwd: Option<&std::path::Path>, profile: Option<&str>) -> Option<String> {
    let config: toml::Table = toml::from_str(config.unwrap_or_default()).ok()?;
    let profile: Option<toml::Table> = match profile {
        Some(text) => Some(toml::from_str(text).ok()?),
        None => None,
    };
    for table in profile.iter().chain([&config]) {
        if table.contains_key("sandbox_mode") {
            return None;
        }
        if let Some(profile) = table.get("default_permissions") {
            return profile.as_str().filter(|p| *p != ":danger-full-access").map(String::from);
        }
    }
    let Some(cwd) = cwd else { return Some(":read-only".into()) };
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    let repo = cwd.ancestors().find(|d| d.join(".git").exists());
    let trusted = config.get("projects").and_then(|p| p.as_table()).is_some_and(|projects| {
        projects.iter().any(|(path, project)| {
            let path = std::path::Path::new(path);
            (path == cwd || Some(path) == repo) && project.get("trust_level").and_then(|t| t.as_str()) == Some("trusted")
        })
    });
    Some(if trusted { ":workspace" } else { ":read-only" }.to_string())
}

/// `text` as a TOML basic string on one line: every control character an
/// escape, so a multi-line prompt survives being typed into a shell.
fn toml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => {}
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A Codex role's prompt as the `-c` override that sets it. Typed inline
/// it makes a line as long as the prompt, and a line over 1 KB typed into
/// a shell is not safe: a tty still in cooked mode — a shell yet to reach
/// its prompt — holds 1024 bytes of a line and drops the rest, and a
/// resize while a line editor works through that much input garbles it.
/// So where the shell can, it goes in a file the shell reads it from
/// ([`launch_command`]'s `prompt_file`).
pub fn codex_instructions(prompt: &str) -> String {
    format!("developer_instructions={}", toml_string(prompt.trim()))
}

/// The program `harness` runs, which is also how a role file names it.
pub fn program_of(harness: Harness) -> &'static str {
    HARNESSES.iter().find(|(_, h)| *h == harness).map(|(p, _)| *p).expect("every harness has a program")
}

/// The harness a role names, by its program (`codex`, `cursor-agent`) or
/// as Mirador spells it (`openCode`, `cursorAgent`), in any case.
pub fn harness_named(name: &str) -> Option<Harness> {
    let name = name.trim().to_ascii_lowercase();
    HARNESSES
        .iter()
        .find(|(program, harness)| {
            *program == name
                || serde_json::to_value(harness)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_ascii_lowercase))
                    .is_some_and(|spelled| spelled == name)
        })
        .map(|(_, harness)| *harness)
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

/// A role's flags for a resumed Claude Code, each already quoted for the
/// shell: its `--model`, `--effort`, settings file and prompt file. A
/// resumed conversation keeps none of the first three — they are the
/// process's — and its prompt only until it is compacted (Claude Code's
/// `--system-prompt-snapshot`), so a role's pane passes them all again or
/// comes back on the default model, with every tool, and later without
/// its role.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResumeFlags<'a> {
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub settings: Option<&'a str>,
    pub prompt: Option<&'a str>,
}

/// What to type into a restored pane to bring its session back. The id is
/// typed into a shell, so anything but a plain id is refused rather than
/// quoted — the session file is not trusted to hold only what we wrote.
/// A daemon session keeps running on whatever it had, and attaching takes
/// none of `flags`.
pub fn restore_command(agent_session: &str, flags: ResumeFlags) -> Option<String> {
    let (agent, id) = agent_session.split_once(':')?;
    if !is_safe_id(id) {
        return None;
    }
    match agent {
        "claude-fg" => {
            let mut line = format!("claude --resume {id}");
            let pairs = [
                ("--model", flags.model),
                ("--effort", flags.effort),
                ("--settings", flags.settings),
                ("--append-system-prompt-file", flags.prompt),
            ];
            for (flag, value) in pairs {
                if let Some(value) = value {
                    line.push_str(&format!(" {flag} {value}"));
                }
            }
            Some(line)
        }
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
/// it is remote, its `agent_session`, its role's `flags` (see
/// [`restore_command`]), and [`AUTO_RESUME`] (a parameter so both
/// platforms' behavior is tested everywhere).
pub fn restore_pane(
    command: Option<&str>,
    remote: bool,
    agent_session: Option<&str>,
    flags: ResumeFlags,
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
    if let Some(input) = restore_command(session, flags) {
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

pub(crate) fn is_safe_id(id: &str) -> bool {
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
    fn finds_panes_running_agents() {
        let procs = parse_ps(PS);
        let mut panes4 = panes();
        panes4.push(("D".into(), 999)); // a pane whose shell is gone
        let running = panes_running_agents(&procs, &panes4);
        let mut found: Vec<&str> = running.keys().map(String::as_str).collect();
        found.sort();
        assert_eq!(found, ["A", "B", "C"]);
        assert_eq!(running["C"].harness, Harness::Claude);
        assert_eq!(running["C"].args, "claude --resume 228fb346-0d12-4618-824c-a0bc432bc2ab");

        let idle = parse_ps("  101 ttys002  -/bin/zsh\n  200 ??  claude daemon run\n");
        assert!(panes_running_agents(&idle, &panes()).is_empty());
    }

    #[test]
    fn finds_other_harnesses_by_the_process_typed_in() {
        // Codex's npm wrapper runs the native binary beneath it, on the same
        // tty; the wrapper is the one that counts. OpenCode's server on
        // its own is not an agent.
        let procs = parse_ps(
            "  101 ttys002  -/bin/zsh
  102 ttys004  -/bin/zsh
  103 ttys006  -/bin/zsh
  300 ttys002  node /opt/homebrew/bin/codex -m gpt-5-codex
  301 ttys002  /opt/homebrew/lib/node_modules/@openai/codex/vendor/aarch64-apple-darwin/codex/codex -m gpt-5-codex
  302 ttys004  node /Users/u/.npm/bin/pi --provider anthropic --model claude-opus-5-5
  303 ttys006  opencode serve --port 4096
",
        );
        let running = panes_running_agents(&procs, &panes());
        assert_eq!(running["A"].harness, Harness::Codex);
        assert!(running["A"].args.starts_with("node "));
        assert_eq!(running["B"].harness, Harness::Pi);
        assert!(!running.contains_key("C"));
    }

    #[test]
    fn names_the_harness_of_a_command_line() {
        let harness = |args: &str| harness_of(args).map(|(h, _)| h);
        assert_eq!(harness("claude"), Some(Harness::Claude));
        assert_eq!(harness("codex"), Some(Harness::Codex));
        assert_eq!(harness("node --no-warnings /usr/local/bin/codex.js exec fix it"), Some(Harness::Codex));
        assert_eq!(harness("/Users/u/.opencode/bin/opencode"), Some(Harness::OpenCode));
        assert_eq!(harness("bun /x/opencode run hi"), Some(Harness::OpenCode));
        assert_eq!(harness("pi"), Some(Harness::Pi));
        assert_eq!(harness("node /opt/homebrew/bin/gemini -m gemini-2.5-pro"), Some(Harness::Gemini));
        assert_eq!(harness("/Users/u/.local/pipx/venvs/aider-chat/bin/python /Users/u/.local/bin/aider"), Some(Harness::Aider));
        assert_eq!(harness("python3.12 -m aider --model sonnet"), Some(Harness::Aider));
        assert_eq!(harness("cursor-agent"), Some(Harness::CursorAgent));
        assert_eq!(harness("C:\\Users\\u\\bin\\codex.exe"), Some(Harness::Codex));
        // Serving something else, not working on anything.
        assert_eq!(harness("codex mcp-server"), None);
        assert_eq!(harness("codex app-server"), None);
        assert_eq!(harness("opencode serve"), None);
        assert_eq!(harness("claude mcp serve"), None);
        // Not an agent, or an agent's name as an argument.
        assert_eq!(harness("node /x/vite.js"), None);
        assert_eq!(harness("vim codex"), None);
        assert_eq!(harness("python3 -c print(1)"), None);
        assert_eq!(harness("node"), None);
        assert_eq!(harness("pip install aider"), None);
    }

    #[test]
    fn records_and_restores_sessions() {
        assert_eq!(session_record("abc-1", None).as_deref(), Some("claude-fg:abc-1"));
        assert_eq!(session_record("abc-1", Some("c5b63b09")).as_deref(), Some("claude-bg:c5b63b09"));
        assert_eq!(session_record("x y", None), None);

        assert_eq!(restore_command("claude-fg:abc-1", ResumeFlags::default()).as_deref(), Some("claude --resume abc-1"));
        assert_eq!(restore_command("claude-bg:c5b63b09", ResumeFlags::default()).as_deref(), Some("claude attach c5b63b09"));
        assert_eq!(restore_command("claude-fg:abc; rm -rf ~", ResumeFlags::default()), None);
        assert_eq!(restore_command("claude-bg:$(id)", ResumeFlags::default()), None);
        assert_eq!(restore_command("other:abc", ResumeFlags::default()), None);
        assert_eq!(restore_command("claude-fg:", ResumeFlags::default()), None);
        // Older records are never auto-typed, only offered on a keypress.
        assert_eq!(restore_command("claude:abc-1", ResumeFlags::default()), None);
        // A role's model, effort, limits and prompt come back with its
        // conversation; an attach takes none of them.
        let role = ResumeFlags {
            model: Some("'opus'"),
            effort: Some("'max'"),
            settings: Some("'/p/r.settings.json'"),
            prompt: Some("'/p/r.md'"),
        };
        assert_eq!(
            restore_command("claude-fg:abc-1", ResumeFlags { model: Some("'opus'"), ..Default::default() }).as_deref(),
            Some("claude --resume abc-1 --model 'opus'")
        );
        assert_eq!(
            restore_command("claude-fg:abc-1", role).as_deref(),
            Some(
                "claude --resume abc-1 --model 'opus' --effort 'max' --settings '/p/r.settings.json' \
                 --append-system-prompt-file '/p/r.md'"
            )
        );
        assert_eq!(
            restore_command("claude-fg:abc-1", ResumeFlags { effort: Some("'high'"), ..Default::default() }).as_deref(),
            Some("claude --resume abc-1 --effort 'high'")
        );
        assert_eq!(
            restore_command("claude-bg:c5b63b09", role).as_deref(),
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
        assert_eq!(restore_pane(None, false, Some("claude-fg:a1"), ResumeFlags::default(), true), typed("claude --resume a1"));
        assert_eq!(restore_pane(None, false, Some("claude-bg:j9"), ResumeFlags::default(), true), typed("claude attach j9"));
        // Windows: idle, a keypress resumes, as before.
        assert_eq!(restore_pane(None, false, Some("claude-fg:a1"), ResumeFlags::default(), false), idle("claude --resume a1"));
        assert_eq!(restore_pane(None, false, Some("claude-bg:j9"), ResumeFlags::default(), false), idle("claude attach j9"));
        // The command Windows saved back is derived again, not kept: once the
        // session is gone the pane is a plain shell.
        assert_eq!(
            restore_pane(Some("claude attach j9"), false, Some("claude-bg:j9"), ResumeFlags::default(), false),
            idle("claude attach j9")
        );
        assert_eq!(restore_pane(Some("claude attach j9"), false, None, ResumeFlags::default(), false), Restored::default());
        assert_eq!(restore_pane(Some("claude --resume a1"), false, None, ResumeFlags::default(), true), Restored::default());
    }

    #[test]
    fn restore_leaves_other_panes_alone() {
        // Legacy records are offered on a keypress on every platform.
        for auto in [true, false] {
            assert_eq!(
                restore_pane(Some("claude --resume old"), false, Some("claude:old"), ResumeFlags::default(), auto),
                Restored { command: Some("claude --resume old".into()), startup_input: None }
            );
        }
        // A real command pane and an SSH pane keep what they had.
        assert_eq!(
            restore_pane(Some("npm test"), false, Some("claude-fg:a1"), ResumeFlags::default(), true),
            Restored { command: Some("npm test".into()), startup_input: None }
        );
        assert_eq!(restore_pane(None, true, Some("claude-fg:a1"), ResumeFlags::default(), true), Restored::default());
        // A plain shell, and a record that is not ours.
        assert_eq!(restore_pane(None, false, None, ResumeFlags::default(), true), Restored::default());
        assert_eq!(restore_pane(None, false, Some("claude-fg:a;b"), ResumeFlags::default(), true), Restored::default());
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
            restore_pane(None, false, Some("claude-fg:a1"), ResumeFlags { model: Some("'opus'"), ..Default::default() }, true),
            Restored { command: None, startup_input: Some("claude --resume a1 --model 'opus'".into()) }
        );
        assert_eq!(
            restore_pane(
                None,
                false,
                Some("claude-fg:a1"),
                ResumeFlags { model: Some("'opus'"), effort: Some("'xhigh'"), prompt: Some("'/p/r.md'"), ..Default::default() },
                true
            ),
            Restored {
                command: None,
                startup_input: Some("claude --resume a1 --model 'opus' --effort 'xhigh' --append-system-prompt-file '/p/r.md'".into())
            }
        );
    }

    #[test]
    fn reads_the_model_flag() {
        assert_eq!(model_flag("claude --model opus --resume x").as_deref(), Some("opus"));
        assert_eq!(model_flag("node /a/claude --model=claude-sonnet-5-5").as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(model_flag("claude --resume x"), None);
        assert_eq!(model_flag("claude --model"), None);
        assert_eq!(model_flag("codex -m gpt-5-codex").as_deref(), Some("gpt-5-codex"));
        assert_eq!(model_flag("opencode -m anthropic/claude-opus-5-5").as_deref(), Some("anthropic/claude-opus-5-5"));
        // `python -m` names the program, not a model.
        assert_eq!(model_flag("python3 -m aider"), None);
        assert_eq!(model_flag("python3 -m aider --model sonnet").as_deref(), Some("sonnet"));
        assert_eq!(model_flag("vim --model x"), None);
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
    fn a_shell_is_busy_while_anything_else_is_on_its_terminal() {
        let procs = parse_ps(PS);
        let alone = parse_ps("  101 ttys009  -/bin/zsh\n  102 ttys010  -/bin/zsh\n  103 ttys010  vim notes.md\n");
        assert!(!shell_busy(&alone, 101));
        assert!(shell_busy(&alone, 102));
        // No such process: refuse rather than type blind.
        assert!(shell_busy(&alone, 999));
        // The PS fixture's shell with `claude` on its terminal is busy.
        let shell = procs.iter().find(|p| p.args.ends_with("zsh") && procs.iter().any(|q| q.pid != p.pid && q.tty == p.tty)).unwrap();
        assert!(shell_busy(&procs, shell.pid));
    }

    #[test]
    fn builds_the_launch_command() {
        let q = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
        assert_eq!(launch_command(None, Harness::Claude, None, None, None, None, None, q), "claude");
        assert_eq!(launch_command(None, Harness::Claude, None, None, None, Some("fix the flaky test"), None, q), "claude 'fix the flaky test'");
        let role = AgentRole {
            name: "reviewer".into(),
            model: Some("opus".into()),
            ..Default::default()
        };
        let prompt = "You review.\nNever edit files; it's read-only.";
        // A prompt file is what Claude Code gets, when there is one.
        let file = std::path::Path::new("/data/prompts/p1.md");
        assert_eq!(
            launch_command(Some(&role), Harness::Claude, Some(prompt), Some(file), None, Some("look at\r\nmain\u{3}"), None, q),
            r"claude --model 'opus' --append-system-prompt-file '/data/prompts/p1.md' --name 'reviewer' 'look at main'"
        );
        assert_eq!(
            launch_command(Some(&role), Harness::Claude, Some(prompt), None, None, None, None, q),
            r"claude --model 'opus' --append-system-prompt 'You review. Never edit files; it'\''s read-only.' --name 'reviewer'"
        );
        // A role that limits its tools says where its settings are.
        let settings = std::path::Path::new("/data/prompts/reviewer-2.settings.json");
        assert_eq!(
            launch_command(Some(&role), Harness::Claude, None, None, Some(settings), None, None, q),
            r"claude --model 'opus' --settings '/data/prompts/reviewer-2.settings.json' --name 'reviewer'"
        );
        let bare = AgentRole { name: "plain".into(), ..Default::default() };
        assert_eq!(launch_command(Some(&bare), Harness::Claude, Some("  "), Some(file), None, Some("  "), None, q), "claude --name 'plain'");
        // Its effort comes right after its model.
        let deep = AgentRole { effort: Some("max".into()), ..role.clone() };
        assert_eq!(
            launch_command(Some(&deep), Harness::Claude, None, None, None, Some("go"), None, q),
            "claude --model 'opus' --effort 'max' --name 'reviewer' 'go'"
        );
    }

    #[test]
    fn starts_plain_codex_when_asked_and_refuses_harnesses_it_cannot_start() {
        let q = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
        assert_eq!(plain_harness(None), Ok(Harness::Claude));
        assert_eq!(plain_harness(Some(Harness::Codex)), Ok(Harness::Codex));
        assert!(plain_harness(Some(Harness::OpenCode)).is_err());
        assert_eq!(launch_command(None, Harness::Codex, None, None, None, Some("go"), None, q), "codex 'go'");
        // It may reach the app as a Codex role may.
        let socket = Some((":read-only", std::path::Path::new("/tmp/mira.sock")));
        assert!(launch_command(None, Harness::Codex, None, None, None, None, socket, q)
            .starts_with("codex -c 'default_permissions=\"mirador\"'"));
        // A role's harness wins over the plain one.
        let claude = AgentRole { name: "reviewer".into(), ..Default::default() };
        assert_eq!(launch_command(Some(&claude), Harness::Codex, None, None, None, None, None, q), "claude --name 'reviewer'");
    }

    #[test]
    fn starts_a_codex_role_with_its_prompt_as_instructions() {
        let q = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
        let role = AgentRole { name: "dev".into(), harness: Harness::Codex, model: Some("gpt-5".into()), ..Default::default() };
        // Without a file, inline; the escapes are TOML's.
        assert_eq!(
            launch_command(Some(&role), Harness::Claude, Some("Line \"one\"\r\n\tC:\\path"), None, None, Some("go"), None, q),
            r#"codex --model 'gpt-5' -c 'developer_instructions="Line \"one\"\n\tC:\\path"' 'go'"#
        );
        // With one, the shell reads the override from it, as one word.
        let file = std::path::Path::new("/data/prompts/dev-1.toml");
        assert_eq!(
            launch_command(Some(&role), Harness::Claude, Some("You build."), Some(file), None, Some("go"), None, q),
            r#"codex --model 'gpt-5' -c "$(command cat '/data/prompts/dev-1.toml')" 'go'"#
        );
        // The override reads back as the prompt.
        let prompt = "a \"b\"\nc\\d\u{7}  e";
        let value: toml::Table = toml::from_str(&codex_instructions(prompt)).unwrap();
        assert_eq!(value["developer_instructions"].as_str(), Some(prompt));
        assert_eq!(launch_command(Some(&role), Harness::Claude, None, None, None, None, None, q), "codex --model 'gpt-5'");
        let deep = AgentRole { effort: Some("xhigh".into()), ..role.clone() };
        assert_eq!(
            launch_command(Some(&deep), Harness::Claude, Some("You build."), None, None, None, None, q),
            r#"codex --model 'gpt-5' -c 'model_reasoning_effort="xhigh"' -c 'developer_instructions="You build."'"#
        );
        // A long prompt makes no long line: a line over 1 KB typed into a
        // shell is cut or garbled (`codex_instructions`).
        let long = "Review every step. ".repeat(270);
        let file = std::path::Path::new("/Users/someone/Library/Application Support/Mirador/prompts/dev-0123456789abcdef.toml");
        let socket = Some((":workspace", std::path::Path::new("/var/folders/xy/abcdefghijklmnop/T/mira-501.sock")));
        let line = launch_command(Some(&deep), Harness::Claude, Some(&long), Some(file), None, None, socket, q);
        assert!(long.len() > 5000 && line.len() < 700, "{} bytes: {line}", line.len());
        // Its profile is the person's own, named as it is.
        let yolo = AgentRole { profile: Some("gpt-6".into()), ..role.clone() };
        assert_eq!(launch_command(Some(&yolo), Harness::Claude, None, None, None, Some("go"), None, q), "codex --model 'gpt-5' --profile 'gpt-6' 'go'");
    }

    #[test]
    fn lets_a_codex_role_reach_miradors_socket_and_nothing_else() {
        let q = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
        let role = AgentRole { name: "dev".into(), harness: Harness::Codex, model: Some("gpt-5".into()), ..Default::default() };
        let socket = Some((":read-only", std::path::Path::new("/var/folders/x/T/mira-501.sock")));
        // The base is what the person's Codex would run on, made no looser.
        assert_eq!(
            launch_command(Some(&role), Harness::Claude, Some("You review."), None, None, None, socket, q),
            "codex --model 'gpt-5' -c 'developer_instructions=\"You review.\"' \
             -c 'default_permissions=\"mirador\"' -c 'permissions.mirador.extends=\":read-only\"' \
             -c 'permissions.mirador.network.unix_sockets={\"/var/folders/x/T/mira-501.sock\"=\"allow\"}' \
             -c 'permissions.mirador.network.enabled=true' -c 'features.network_proxy=true'"
        );
        // Claude Code is started as before.
        let claude = AgentRole { name: "reviewer".into(), ..Default::default() };
        assert_eq!(launch_command(Some(&claude), Harness::Claude, None, None, None, None, socket, q), "claude --name 'reviewer'");
    }

    #[test]
    fn extends_the_codex_profile_the_person_runs_on() {
        let dir = std::env::temp_dir().join(format!("mira-codex-base-{}", std::process::id()));
        let project = dir.join("repo/sub");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(dir.join("repo/.git")).unwrap();
        let repo = dir.join("repo").canonicalize().unwrap();
        let base = |config: Option<&str>| codex_base_profile(config, Some(&project), None);

        // Their own profile, built-in or theirs: Codex reads its definition.
        assert_eq!(base(Some("default_permissions = \":read-only\"")).as_deref(), Some(":read-only"));
        assert_eq!(base(Some("default_permissions = \"strict\"\n[permissions.strict]\nextends = \":read-only\"")).as_deref(), Some("strict"));
        // None named: Codex's default, by whether the repository is trusted.
        let trusted = format!("[projects.{:?}]\ntrust_level = \"trusted\"", repo.to_string_lossy());
        assert_eq!(base(Some(&trusted)).as_deref(), Some(":workspace"));
        assert_eq!(base(Some(&trusted.replace("trusted\"", "untrusted\""))).as_deref(), Some(":read-only"));
        assert_eq!(base(None).as_deref(), Some(":read-only"));
        assert_eq!(codex_base_profile(Some(&trusted), None, None).as_deref(), Some(":read-only"), "nowhere known");
        // Nothing added where it cannot be done without guessing.
        assert_eq!(base(Some("default_permissions = ")), None, "a config that does not parse");
        assert_eq!(base(Some("sandbox_mode = \"read-only\"")), None, "profiles are ignored then");
        assert_eq!(base(Some("default_permissions = \":danger-full-access\"")), None);

        // The role's Codex profile, its own file, wins over config.toml.
        let in_profile = |config: &str, profile: &str| codex_base_profile(Some(config), Some(&project), Some(profile));
        let top = "default_permissions = \":read-only\"";
        assert_eq!(in_profile(top, "sandbox_mode = \"danger-full-access\""), None, "its sandbox_mode");
        assert_eq!(in_profile(top, "default_permissions = \"locked\"").as_deref(), Some("locked"));
        assert_eq!(in_profile(&trusted, "model = \"gpt-6\"").as_deref(), Some(":workspace"), "silent on it");
        assert_eq!(in_profile(top, "model = "), None, "a profile that does not parse");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn names_a_harness_as_people_write_it() {
        assert_eq!(harness_named("codex"), Some(Harness::Codex));
        assert_eq!(harness_named(" Claude "), Some(Harness::Claude));
        assert_eq!(harness_named("openCode"), Some(Harness::OpenCode));
        assert_eq!(harness_named("cursor-agent"), Some(Harness::CursorAgent));
        assert_eq!(harness_named("cursorAgent"), Some(Harness::CursorAgent));
        assert_eq!(harness_named("vim"), None);
    }

    #[test]
    fn forgets_an_agent_that_exited_or_never_started() {
        use std::time::Duration;
        let long = Some(START_GRACE + Duration::from_secs(1));
        let short = Some(Duration::from_secs(3));
        // Running: never forgotten, however long ago it started.
        assert!(!forget_agent(true, true, false, long));
        // Ran, then stopped: exited.
        assert!(forget_agent(false, true, false, None));
        // A resume still to be typed: not yet.
        assert!(!forget_agent(false, false, true, long));
        // Never seen: given the grace period, then let go.
        assert!(!forget_agent(false, false, false, short));
        assert!(forget_agent(false, false, false, long));
        // Never seen and never started by us (a hook-recorded session the
        // poller has not caught yet): left alone.
        assert!(!forget_agent(false, false, false, None));
    }

    #[test]
    fn reads_claude_code_session_files() {
        let s = parse_claude_session(
            r#"{"pid":79027,"sessionId":"c299bda0","name":"Default path\nfor panes","nameSource":"auto","jobId":"c5b63b09","status":"busy","kind":"bg"}"#,
        )
        .unwrap();
        assert_eq!(s.pid, 79027);
        assert_eq!(s.name.as_deref(), Some("Default path for panes"));
        assert_eq!(s.job_id.as_deref(), Some("c5b63b09"));
        assert_eq!(s.status, Some(AgentStatus::Working));
        let idle = parse_claude_session(r#"{"pid":1,"status":"idle","name":"  "}"#).unwrap();
        assert_eq!((idle.status, idle.name), (Some(AgentStatus::Idle), None));
        assert_eq!(parse_claude_session(r#"{"pid":1,"status":"thinking"}"#).unwrap().status, None);
        assert_eq!(parse_claude_session(r#"{"name":"no pid"}"#), None);
        assert_eq!(parse_claude_session("not json"), None);
    }

    #[test]
    fn finds_the_session_of_a_pane() {
        let sessions = [
            ClaudeSession { pid: 10, session_id: Some("s-fg".into()), name: Some("fg".into()), ..Default::default() },
            ClaudeSession { pid: 20, session_id: Some("s-bg".into()), job_id: Some("job9".into()), name: Some("bg".into()), ..Default::default() },
        ];
        let name = |found: Option<&ClaudeSession>| found.and_then(|s| s.name.clone());
        // Its own process decides first.
        assert_eq!(name(find_claude_session(&sessions, Some(10), Some("claude-fg:s-bg"), None)).as_deref(), Some("fg"));
        // Then what the hooks recorded.
        assert_eq!(name(find_claude_session(&sessions, Some(99), Some("claude-fg:s-fg"), None)).as_deref(), Some("fg"));
        assert_eq!(name(find_claude_session(&sessions, None, Some("claude-bg:job9"), None)).as_deref(), Some("bg"));
        // An attach with no hooks, by its job.
        assert_eq!(name(find_claude_session(&sessions, Some(99), None, Some("claude attach job9"))).as_deref(), Some("bg"));
        // A job id is not a session id.
        assert_eq!(find_claude_session(&sessions, None, Some("claude-fg:job9"), None), None);
        assert_eq!(find_claude_session(&sessions, None, None, None), None);
    }

    #[test]
    fn finds_the_sessions_directory() {
        use std::path::Path;
        let t = Path::new("/Users/u/.claude/projects/-Users-u-w/abc.jsonl");
        assert_eq!(sessions_dir(Some(t)).as_deref(), Some(Path::new("/Users/u/.claude/sessions")));
        let custom = Path::new("/cfg/projects/p/abc.jsonl");
        assert_eq!(sessions_dir(Some(custom)).as_deref(), Some(Path::new("/cfg/sessions")));
        // Not a transcript where Claude Code keeps them: the default.
        let odd = sessions_dir(Some(Path::new("/tmp/abc.jsonl")));
        assert_eq!(odd, crate::claude_integration::claude_dir().map(|d| d.join("sessions")));
    }

    #[test]
    fn a_title_names_what_the_agent_is_on() {
        let claude = Harness::Claude;
        assert_eq!(title_topic("✳ Agent wall tile column logic", claude, &[]).as_deref(), Some("Agent wall tile column logic"));
        assert_eq!(title_topic("⠂ Fix the flaky test", claude, &[Some("coder")]).as_deref(), Some("Fix the flaky test"));
        // The role, the session's name, the harness's, or nothing: no label.
        assert_eq!(title_topic("✳ reviewer", claude, &[Some("reviewer")]), None);
        assert_eq!(title_topic("✳ Fix login", claude, &[None, Some("fix login")]), None);
        assert_eq!(title_topic("✳ Claude Code", claude, &[]), None);
        assert_eq!(title_topic("  ✶  ", claude, &[]), None);
        assert_eq!(title_topic("OpenCode", Harness::OpenCode, &[]), None);
        assert_eq!(title_topic("codex", Harness::Codex, &[]), None);
        // Another agent's name is not this one's role, and is kept.
        assert_eq!(title_topic("✳ planner", claude, &[Some("reviewer")]).as_deref(), Some("planner"));
    }

    fn agent() -> AgentInfo {
        AgentInfo {
            pane_id: "p".into(),
            tab_id: "t".into(),
            harness: Harness::Claude,
            role: None,
            member: None,
            has_activity: false,
            name: None,
            model: None,
            effort: None,
            status: None,
            message: None,
            asking: None,
            topic: None,
            since_ms: None,
            cwd: None,
            branch: None,
        }
    }

    #[test]
    fn an_agent_is_labelled_by_member_then_session_then_role_then_harness() {
        let mut a = agent();
        assert_eq!(label(&a), "Claude Code");
        a.role = Some("dev".into());
        assert_eq!(label(&a), "dev");
        a.name = Some("fix-login".into());
        assert_eq!(label(&a), "fix-login");
        a.member = Some("reviewer".into());
        assert_eq!(label(&a), "reviewer");
    }

    #[test]
    fn an_agents_notification_names_the_agent_and_its_tab() {
        let mut a = agent();
        a.member = Some("reviewer".into());
        // Claude Code's own alerts, titled with nothing but its name.
        assert_eq!(
            notification_title(Some(&a), "api", Some("Claude Code".into())).as_deref(),
            Some("reviewer · api")
        );
        assert_eq!(notification_title(Some(&agent()), "docs", None).as_deref(), Some("Claude Code · docs"));
    }

    #[test]
    fn any_other_panes_notification_keeps_its_title() {
        assert_eq!(notification_title(None, "api", Some("Build done".into())).as_deref(), Some("Build done"));
        assert_eq!(notification_title(None, "api", None), None);
    }
}
