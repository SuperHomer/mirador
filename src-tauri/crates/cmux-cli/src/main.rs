//! `mira` CLI — drives the running app over its automation socket.
//! `notify` also works without the socket (prints an OSC 777 escape, so it
//! reaches Mirador through any nesting, including SSH).

use std::io::Write;

use clap::{Parser, Subcommand};
use cmux_protocol::{Request, ResponseEnvelope, SplitDir};

#[derive(Parser)]
#[command(name = "mira", about = "Automation client for the Mirador terminal")]
struct Cli {
    /// Print raw JSON responses.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List tabs and panes (workspace snapshot).
    ListTabs,
    /// Open a new tab, optionally running a command in it.
    NewTab {
        #[arg(short, long)]
        command: Option<String>,
    },
    /// Split a pane (the focused one unless --pane is given).
    Split {
        #[arg(long, default_value = "row")]
        dir: String,
        #[arg(long)]
        pane: Option<String>,
        #[arg(short, long)]
        command: Option<String>,
    },
    /// Close a pane.
    ClosePane { pane: String },
    /// Focus a pane (activates its tab).
    Focus { pane: String },
    /// Show a pane alone, filling its tab (activates it); again, or with
    /// --off, shows the whole tab. PANE defaults to the focused pane.
    Zoom {
        pane: Option<String>,
        #[arg(long)]
        off: bool,
    },
    /// Type input into a pane's shell.
    SendInput {
        /// Text to send (use --enter to append a newline).
        data: String,
        #[arg(long)]
        pane: Option<String>,
        /// Append a newline (press Enter).
        #[arg(long)]
        enter: bool,
    },
    /// Read a pane's screen contents as plain text.
    ReadScreen {
        #[arg(long)]
        pane: Option<String>,
        /// Trailing buffer lines to read (default: visible screen).
        #[arg(long)]
        lines: Option<u32>,
    },
    /// Run a command in a visible command pane (split of the focused pane,
    /// or a new tab with --tab). Humans watch the same execution the agent
    /// reads; Ctrl-C in the pane interrupts it for both.
    Run {
        /// Open the command in a new tab instead of a split.
        #[arg(long)]
        tab: bool,
        /// Block until the command exits: prints its clean output and
        /// exits with the command's exit code.
        #[arg(long)]
        wait: bool,
        /// With --wait: don't reprint the output (you're watching the
        /// pane); still adopts the exit code.
        #[arg(short, long)]
        quiet: bool,
        /// Seconds to wait before giving up (default 600).
        #[arg(long)]
        timeout: Option<u64>,
        #[arg(trailing_var_arg = true, required = true)]
        command: Vec<String>,
    },
    /// Command-pane run history (what ran, when, exit codes).
    Runs,
    /// Drive the built-in browser pane (agents verify web changes here).
    Browser {
        #[command(subcommand)]
        action: BrowserAction,
    },
    /// Open a diff pane: a reviewable view of what changed, with a file
    /// tree — the repository comes from the pane you run it in.
    Diff {
        /// A commit ("0e47a2c"), a range ("main...HEAD"), or nothing for
        /// the uncommitted changes.
        spec: Option<String>,
        /// Diff the index against HEAD instead of the working tree.
        #[arg(long)]
        staged: bool,
        /// Open in a new tab instead of a split.
        #[arg(long)]
        tab: bool,
        /// Review another checkout of this repository, by branch name
        /// ("feature/login") or path. Defaults to the calling pane's.
        #[arg(long, value_name = "BRANCH|PATH")]
        worktree: Option<String>,
        /// List this repository's worktrees instead of opening a pane.
        #[arg(long, conflicts_with_all = ["spec", "staged", "tab", "worktree"])]
        list_worktrees: bool,
    },
    /// Open the commit graph for this pane's repository.
    Graph {
        /// Open in a new tab instead of a split.
        #[arg(long)]
        tab: bool,
    },
    /// Claude Code agents: start one with a role, list them, watch them all.
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },
    /// Remote workspaces over SSH.
    Ssh {
        #[command(subcommand)]
        action: SshAction,
    },
    /// Send a notification (prints OSC 777; works from inside any pane,
    /// even over SSH). Use --socket to target the app directly instead.
    Notify {
        #[arg(short, long)]
        title: Option<String>,
        #[arg(long)]
        socket: bool,
        #[arg(trailing_var_arg = true, required = true)]
        body: Vec<String>,
    },
    /// Claude Code hook handler (wired up by `mira hooks setup`); reads
    /// the hook payload from stdin.
    #[command(hide = true)]
    ClaudeHook { event: String },
    /// Install or remove the Claude Code integration: the hooks that light
    /// up Mirador tabs, and the `/mira-diff` slash command.
    Hooks {
        /// "setup" or "remove"
        action: String,
    },
    /// Quit Mirador. With `persistSessions` on, terminals keep running for
    /// the next launch unless --end-sessions ends them first.
    Quit {
        #[arg(long)]
        end_sessions: bool,
    },
    /// Put `mira` on your PATH: a symlink in ~/.local/bin on macOS and
    /// Linux; on Windows, the install directory joins your user PATH.
    Install,
    /// Session holder: owns one pane's terminal so it outlives the app.
    /// Started by Mirador, not by hand — see docs/design/session-persistence.md.
    #[command(name = "__hold", hide = true)]
    Hold {
        #[arg(long)]
        pane: String,
        #[arg(long)]
        socket: std::path::PathBuf,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long, default_value_t = 80)]
        cols: u16,
        #[arg(long, default_value_t = 24)]
        rows: u16,
        /// Run this command line instead of an interactive shell.
        #[arg(long, conflicts_with = "ssh")]
        command: Option<String>,
        /// Run `ssh -tt <HOST>` instead of an interactive shell.
        #[arg(long)]
        ssh: Option<String>,
    },
}

#[derive(Subcommand)]
enum AgentAction {
    /// Start Claude Code in a new tab, with a role's model and prompt from
    /// `agentRoles` in mirador.json. TASK becomes its first message.
    New {
        /// A role name from `agentRoles` (`mira agent roles` lists them).
        #[arg(short, long)]
        role: Option<String>,
        /// Split this pane instead of opening a tab.
        #[arg(long)]
        split: bool,
        #[arg(trailing_var_arg = true)]
        task: Vec<String>,
    },
    /// Every agent pane: role, model, and what it is doing.
    List,
    /// The roles `agent new --role` accepts.
    Roles,
    /// Open the agent wall — every agent's terminal in one grid. Goes to
    /// the existing wall if there is one.
    Wall {
        /// Split this pane instead of opening (or going to) a tab.
        #[arg(long)]
        split: bool,
    },
}

#[derive(Subcommand)]
enum SshAction {
    /// Open a remote pane running `ssh -tt <host>`. HOST is an ssh alias
    /// (from ~/.ssh/config) or a full destination, optionally with ssh
    /// options: "-p 2222 user@box".
    Open {
        /// Hyphens allowed so a full spec like "-p 2222 user@box" works.
        #[arg(allow_hyphen_values = true)]
        host: String,
        /// Open in a new tab instead of a split.
        #[arg(long)]
        tab: bool,
    },
    /// List host aliases from ~/.ssh/config.
    Hosts,
    /// Forward localhost:PORT to the remote's localhost:PORT (so the
    /// browser pane can reach a remote dev server).
    Forward {
        port: u16,
        #[arg(long)]
        pane: Option<String>,
    },
    /// Cancel a forward established with `ssh forward`.
    Unforward {
        port: u16,
        #[arg(long)]
        pane: Option<String>,
    },
}

#[derive(Subcommand)]
enum BrowserAction {
    /// Open a browser pane (split of the focused pane, or --tab).
    Open {
        url: String,
        #[arg(long)]
        tab: bool,
    },
    /// Navigate the browser pane to a URL.
    Navigate {
        url: String,
        #[arg(long)]
        pane: Option<String>,
    },
    /// Accessibility-style snapshot of the page (elements with stable ids).
    Snapshot {
        #[arg(long)]
        pane: Option<String>,
    },
    /// Click an element by snapshot id or CSS selector.
    Click {
        target: String,
        #[arg(long)]
        pane: Option<String>,
    },
    /// Fill an input (snapshot id or CSS selector) with a value.
    Fill {
        target: String,
        value: String,
        #[arg(long)]
        pane: Option<String>,
    },
    /// Evaluate JavaScript in the page; prints the JSON result.
    Eval {
        js: String,
        #[arg(long)]
        pane: Option<String>,
    },
    Back {
        #[arg(long)]
        pane: Option<String>,
    },
    Forward {
        #[arg(long)]
        pane: Option<String>,
    },
    Reload {
        #[arg(long)]
        pane: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("mira: {e}");
            std::process::exit(1);
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    let mut quiet_output = false;
    let req = match cli.command {
        Command::ListTabs => Request::ListTabs,
        Command::NewTab { command } => Request::NewTab { command },
        Command::Split { dir, pane, command } => Request::SplitPane {
            pane_id: pane,
            dir: parse_dir(&dir)?,
            command,
        },
        Command::ClosePane { pane } => Request::ClosePane { pane_id: pane },
        Command::Focus { pane } => Request::FocusPane { pane_id: pane },
        Command::Zoom { pane, off } => Request::ZoomPane {
            pane_id: pane,
            zoom: off.then_some(false),
        },
        Command::SendInput { data, pane, enter } => Request::SendInput {
            pane_id: pane,
            data: if enter { format!("{data}\n") } else { data },
        },
        Command::ReadScreen { pane, lines } => Request::ReadScreen {
            pane_id: pane,
            lines,
        },
        Command::Run {
            tab,
            wait,
            quiet,
            timeout,
            command,
        } => {
            quiet_output = quiet;
            Request::Run {
                command: command.join(" "),
                target: tab.then(|| "tab".to_string()),
                wait,
                timeout_secs: timeout,
            }
        }
        Command::Runs => Request::ListRuns,
        Command::Diff {
            spec,
            staged,
            tab,
            worktree,
            list_worktrees,
        } => {
            if staged && spec.is_some() {
                return Err("--staged takes no revision".into());
            }
            if list_worktrees {
                // A listing only has to find a repository, and falling back
                // to the focused pane cannot write to the wrong one.
                Request::DiffWorktrees {
                    pane_id: own_pane(),
                }
            } else {
                // Without a pane we cannot tell which repository is meant.
                // The app would fall back to whichever pane is focused,
                // which from another terminal silently diffs someone else's
                // work — so refuse instead of guessing.
                let pane_id = own_pane().ok_or(
                    "`mira diff` must run inside a Mirador pane: it takes the \
                     repository from the pane it was called in, and this shell \
                     is not one (MIRA_PANE is unset; `sudo` and detached \
                     sessions drop it too). From the app, the command palette's \
                     \"New Diff Pane\" does the same thing.",
                )?;
                Request::DiffOpen {
                    spec: if staged {
                        Some("staged".to_string())
                    } else {
                        spec
                    },
                    target: tab.then(|| "tab".to_string()),
                    pane_id: Some(pane_id),
                    worktree,
                }
            }
        }
        Command::Graph { tab } => {
            // Same rule as `mira diff`: the repository comes from the pane
            // this ran in, so from another terminal there is nothing to
            // guess from and guessing would show someone else's history.
            let pane_id = own_pane().ok_or(
                "`mira graph` must run inside a Mirador pane: it takes the \
                 repository from the pane it was called in, and this shell \
                 is not one (MIRA_PANE is unset). From the app, the command \
                 palette's \"Git: Commit Graph\" does the same thing.",
            )?;
            Request::GraphOpen {
                target: tab.then(|| "tab".to_string()),
                pane_id: Some(pane_id),
            }
        }
        Command::Agent { action } => match action {
            AgentAction::New { role, split, task } => Request::AgentNew {
                role,
                task: (!task.is_empty()).then(|| task.join(" ")),
                target: split.then(|| "split".to_string()),
                pane_id: own_pane(),
            },
            AgentAction::List => Request::AgentList,
            AgentAction::Roles => return agent_roles(cli.json),
            AgentAction::Wall { split } => Request::AgentWall {
                target: split.then(|| "split".to_string()),
                pane_id: own_pane(),
            },
        },
        Command::Ssh { action } => match action {
            SshAction::Open { host, tab } => Request::SshOpen {
                host,
                target: tab.then(|| "tab".to_string()),
            },
            SshAction::Hosts => Request::SshHosts,
            SshAction::Forward { port, pane } => Request::SshForward {
                pane_id: pane,
                port,
                cancel: false,
            },
            SshAction::Unforward { port, pane } => Request::SshForward {
                pane_id: pane,
                port,
                cancel: true,
            },
        },
        Command::Browser { action } => match action {
            BrowserAction::Open { url, tab } => Request::BrowserOpen {
                url,
                target: tab.then(|| "tab".to_string()),
            },
            BrowserAction::Navigate { url, pane } => Request::BrowserNavigate {
                pane_id: pane,
                url,
            },
            BrowserAction::Snapshot { pane } => Request::BrowserSnapshot { pane_id: pane },
            BrowserAction::Click { target, pane } => Request::BrowserClick {
                pane_id: pane,
                target,
            },
            BrowserAction::Fill {
                target,
                value,
                pane,
            } => Request::BrowserFill {
                pane_id: pane,
                target,
                value,
            },
            BrowserAction::Eval { js, pane } => Request::BrowserEval { pane_id: pane, js },
            BrowserAction::Back { pane } => Request::BrowserHistory {
                pane_id: pane,
                action: "back".into(),
            },
            BrowserAction::Forward { pane } => Request::BrowserHistory {
                pane_id: pane,
                action: "forward".into(),
            },
            BrowserAction::Reload { pane } => Request::BrowserHistory {
                pane_id: pane,
                action: "reload".into(),
            },
        },
        Command::Notify {
            title,
            socket,
            body,
        } => {
            let body = body.join(" ");
            if !socket {
                return print_osc_notify(title.as_deref(), &body);
            }
            Request::Notify {
                pane_id: own_pane(),
                tty: own_tty(),
                title,
                body,
                job: own_job(),
            }
        }
        Command::ClaudeHook { event } => return claude_hook(&event),
        Command::Hooks { action } => return hooks(&action),
        Command::Install => return install(),
        Command::Quit { end_sessions } => Request::Quit { end_sessions },
        Command::Hold {
            pane,
            socket,
            cwd,
            cols,
            rows,
            command,
            ssh,
        } => return hold(pane, socket, cwd, cols, rows, command, ssh),
    };

    let response = send_request(req)?;
    render(response, cli.json, quiet_output)
}

fn parse_dir(s: &str) -> Result<SplitDir, String> {
    match s {
        "row" | "right" | "horizontal" => Ok(SplitDir::Row),
        "column" | "down" | "vertical" => Ok(SplitDir::Column),
        other => Err(format!("invalid direction `{other}` (row|column)")),
    }
}

fn send_request(req: Request) -> Result<ResponseEnvelope, String> {
    use cmux_protocol::RequestEnvelope;
    use std::io::{BufRead, BufReader};

    let disc = cmux_core::ipc::read_discovery()
        .ok_or("Mirador is not running (no socket discovery file)")?;
    let stream = cmux_core::transport::connect(&disc.socket)
        .map_err(|e| format!("Mirador is not running ({e})"))?;

    let envelope = RequestEnvelope { id: Some(1), req };
    let mut line = serde_json::to_vec(&envelope).map_err(|e| e.to_string())?;
    line.push(b'\n');

    let mut reader = BufReader::new(stream);
    reader.get_mut().write_all(&line).map_err(|e| e.to_string())?;
    reader.get_mut().flush().map_err(|e| e.to_string())?;

    let mut response = String::new();
    reader
        .read_line(&mut response)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&response).map_err(|e| format!("bad response: {e}"))
}

fn render(resp: ResponseEnvelope, raw_json: bool, quiet: bool) -> Result<(), String> {
    if !resp.ok {
        return Err(resp.error.unwrap_or_else(|| "unknown error".into()));
    }
    let data = resp.data.unwrap_or(serde_json::Value::Null);
    if raw_json {
        println!("{}", serde_json::to_string_pretty(&data).unwrap());
        return Ok(());
    }
    match &data {
        serde_json::Value::Null => {}
        serde_json::Value::Object(map) => {
            if map.contains_key("output") {
                // run --wait: print the command's output, adopt its exit code.
                if !quiet {
                    if let Some(out) = map.get("output").and_then(|v| v.as_str()) {
                        println!("{out}");
                    }
                }
                let code = map.get("exitCode").and_then(|v| v.as_i64()).unwrap_or(0);
                std::process::exit(code as i32);
            } else if let Some(text) = map.get("text").and_then(|v| v.as_str()) {
                println!("{text}");
            } else if map.contains_key("nodes") {
                render_page_snapshot(&data);
            } else if map.contains_key("value") {
                println!("{}", serde_json::to_string_pretty(&map["value"]).unwrap());
            } else if let Some(hosts) = map.get("hosts").and_then(|v| v.as_array()) {
                if hosts.is_empty() {
                    println!("(no Host entries in ~/.ssh/config)");
                }
                for h in hosts {
                    println!("{}", h.as_str().unwrap_or(""));
                }
            } else if map.contains_key("runs") {
                render_runs(&data);
            } else if let Some(agents) = map
                .get("agents")
                .and_then(|v| v.as_array())
                // A workspace snapshot carries its agents too.
                .filter(|_| !map.contains_key("tabs"))
            {
                render_agents(agents);
            } else if let Some(id) = map
                .get("paneId")
                .or_else(|| map.get("tabId"))
                .and_then(|v| v.as_str())
            {
                println!("{id}");
            } else if map.contains_key("tabs") {
                render_tabs(&data);
            } else {
                println!("{}", serde_json::to_string_pretty(&data).unwrap());
            }
        }
        // `diff --list-worktrees` answers with an array.
        serde_json::Value::Array(items)
            if items.first().is_some_and(|i| i.get("path").is_some()) =>
        {
            render_worktrees(items);
        }
        other => println!("{}", serde_json::to_string_pretty(other).unwrap()),
    }
    Ok(())
}

/// Agents as a table, one per line: pane, status, role, model, and what it
/// is blocked on when it is.
fn render_agents(agents: &[serde_json::Value]) {
    if agents.is_empty() {
        println!("(no agents — `mira agent new` starts one)");
        return;
    }
    for a in agents {
        let status = match a["status"].as_str() {
            Some("working") => "working",
            Some("needsYou") => "needs-you",
            Some("idle") => "idle",
            _ => "-",
        };
        let mut line = format!(
            "{}  {:<9}  {:<12}  {}",
            a["paneId"].as_str().unwrap_or(""),
            status,
            a["role"].as_str().unwrap_or("-"),
            a["model"].as_str().unwrap_or("default"),
        );
        if let Some(message) = a["message"].as_str() {
            line.push_str(&format!("  — {message}"));
        }
        println!("{line}");
    }
}

/// `mira agent roles`: read from the config file, as the app reads it — no
/// running app needed.
fn agent_roles(json: bool) -> Result<(), String> {
    let roles = cmux_core::config::agent_roles();
    if json {
        println!("{}", serde_json::to_string_pretty(&roles).map_err(|e| e.to_string())?);
        return Ok(());
    }
    if roles.is_empty() {
        println!(
            "(no roles — add \"agentRoles\" to {})",
            cmux_core::config::config_path().display()
        );
    }
    for r in roles {
        println!("{:<12}  {}", r.name, r.model.as_deref().unwrap_or("default"));
    }
    Ok(())
}

/// Worktrees as a table: the current one marked, so `mira diff --worktree`
/// can be fed straight from the first column.
fn render_worktrees(items: &[serde_json::Value]) {
    let name = |w: &serde_json::Value| match w["branch"].as_str() {
        Some(branch) => branch.to_string(),
        // Detached or bare: there is no branch to name it by, so show what
        // there is — and it is still selectable by path.
        None => match w["head"].as_str() {
            Some(head) => format!("(detached {head})"),
            None => "(bare)".to_string(),
        },
    };
    let width = items.iter().map(|w| name(w).chars().count()).max().unwrap_or(0);
    for w in items {
        let mut tags: Vec<&str> = Vec::new();
        if w["main"].as_bool() == Some(true) {
            tags.push("main");
        }
        if w["locked"].as_bool() == Some(true) {
            tags.push("locked");
        }
        if w["prunable"].as_bool() == Some(true) {
            tags.push("prunable");
        }
        let suffix = if tags.is_empty() {
            String::new()
        } else {
            format!("  [{}]", tags.join(", "))
        };
        println!(
            "{} {:width$}  {}{}",
            if w["current"].as_bool() == Some(true) { "*" } else { " " },
            name(w),
            w["path"].as_str().unwrap_or(""),
            suffix,
        );
    }
}

fn render_page_snapshot(data: &serde_json::Value) {
    println!(
        "{} — {}",
        data["title"].as_str().unwrap_or(""),
        data["url"].as_str().unwrap_or("")
    );
    for node in data["nodes"].as_array().unwrap_or(&Vec::new()) {
        let mut extras = Vec::new();
        if let Some(t) = node["type"].as_str() {
            extras.push(format!("type={t}"));
        }
        if let Some(v) = node["value"].as_str() {
            extras.push(format!("value=\"{v}\""));
        }
        if let Some(h) = node["href"].as_str() {
            extras.push(format!("href={h}"));
        }
        if node["checked"].as_bool() == Some(true) {
            extras.push("checked".into());
        }
        if node["disabled"].as_bool() == Some(true) {
            extras.push("disabled".into());
        }
        println!(
            "[{}] <{}> \"{}\"{}{}",
            node["id"],
            node["tag"].as_str().unwrap_or(""),
            node["text"].as_str().unwrap_or(""),
            if extras.is_empty() { "" } else { " " },
            extras.join(" ")
        );
    }
}

fn render_runs(data: &serde_json::Value) {
    for run in data["runs"].as_array().unwrap_or(&Vec::new()) {
        let status = match (run["finishedMs"].as_u64(), run["exitCode"].as_i64()) {
            (None, _) => "running".to_string(),
            (Some(_), Some(0)) => "ok".to_string(),
            (Some(_), Some(code)) => format!("exit {code}"),
            (Some(_), None) => "finished".to_string(),
        };
        let duration = match (run["startedMs"].as_u64(), run["finishedMs"].as_u64()) {
            (Some(s), Some(f)) => format!("{:.1}s", (f.saturating_sub(s)) as f64 / 1000.0),
            _ => "…".to_string(),
        };
        println!(
            "{:<10} {:>8}  {}  [{}]",
            status,
            duration,
            run["command"].as_str().unwrap_or(""),
            run["paneId"].as_str().unwrap_or(""),
        );
    }
}

fn render_tabs(data: &serde_json::Value) {
    let active = data["activeTab"].as_str().unwrap_or("");
    for tab in data["tabs"].as_array().unwrap_or(&Vec::new()) {
        let marker = if tab["id"] == active { "*" } else { " " };
        println!(
            "{} {}  {}  [{}]",
            marker,
            tab["id"].as_str().unwrap_or(""),
            tab["title"].as_str().unwrap_or(""),
            tab["cwd"].as_str().unwrap_or(""),
        );
        print_panes(&tab["root"], tab["focusedPane"].as_str().unwrap_or(""), 4);
    }
}

fn print_panes(node: &serde_json::Value, focused: &str, indent: usize) {
    if let Some(pane) = node["paneId"].as_str() {
        let marker = if pane == focused { "▸" } else { " " };
        println!("{}{} pane {}", " ".repeat(indent), marker, pane);
    }
    if let Some(children) = node["children"].as_array() {
        for child in children {
            print_panes(child, focused, indent + 2);
        }
    }
}

fn print_osc_notify(title: Option<&str>, body: &str) -> Result<(), String> {
    let clean = |s: &str| s.chars().filter(|c| !c.is_control()).collect::<String>();
    let mut out = std::io::stdout();
    write!(
        out,
        "\x1b]777;notify;{};{}\x1b\\",
        clean(title.unwrap_or("")),
        clean(body)
    )
    .map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())
}

/// The pane we are running inside, from the environment every pane's
/// processes inherit. Works on every platform and through nesting (tmux,
/// subshells, agent hooks). When the environment is lost — `sudo`, a
/// detached screen — unix falls back to the tty lookup below; from a remote
/// host neither applies and the OSC form of `mira notify` is the answer.
fn own_pane() -> Option<String> {
    std::env::var(cmux_core::pty::PANE_ENV)
        .ok()
        .filter(|id| !id.is_empty())
}

/// The tty of our parent process (the shell/agent inside a Mirador pane) —
/// our own stdio may be pipes when invoked as a hook.
#[cfg(unix)]
fn own_tty() -> Option<String> {
    let ppid = std::os::unix::process::parent_id();
    let output = std::process::Command::new("ps")
        .args(["-o", "tty=", "-p", &ppid.to_string()])
        .output()
        .ok()?;
    let tty = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if tty.is_empty() || tty.contains('?') {
        None
    } else {
        Some(tty)
    }
}

#[cfg(not(unix))]
fn own_tty() -> Option<String> {
    None
}

fn hold(
    pane: String,
    socket: std::path::PathBuf,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    command: Option<String>,
    ssh: Option<String>,
) -> Result<(), String> {
    use cmux_core::hold::server::{serve, HoldConfig, Program};
    // Nothing inherited survives: on macOS a socket becomes close-on-exec
    // only after it exists, so a holder launched while the app was opening
    // a connection to *another* holder can inherit that connection — and,
    // living for days, keep it open after the app quits, so that holder
    // thinks a client is still reading and its child blocks on a full
    // socket. Nothing above stderr is ours yet, so all of it goes.
    #[cfg(unix)]
    close_inherited_fds();
    // A session of its own: no controlling terminal to hang it up, and out
    // of the app's process group, so neither the app quitting nor a signal
    // to its group reaches the shell this holds. (On Windows the app starts
    // the holder detached instead — see `hold::launch` — and handles are
    // not inherited unless marked so, which none of the app's are.)
    #[cfg(unix)]
    unsafe {
        libc::setsid();
    }
    let program = match (command, ssh) {
        (Some(cmd), _) => Program::Command(cmd),
        (None, Some(host)) => Program::Ssh(host),
        (None, None) => Program::Shell,
    };
    let code = serve(HoldConfig {
        pane,
        socket,
        cwd,
        cols,
        rows,
        program,
        replay_bytes: HoldConfig::DEFAULT_REPLAY_BYTES,
        exited_grace: HoldConfig::DEFAULT_EXITED_GRACE,
    })
    .map_err(|e| e.to_string())?;
    std::process::exit(code.unwrap_or(1));
}

/// Closes every descriptor above stderr. Runs first thing in a holder,
/// before any thread exists or any file of its own is open.
#[cfg(unix)]
fn close_inherited_fds() {
    let max = match unsafe { libc::sysconf(libc::_SC_OPEN_MAX) } {
        n if n > 0 => (n as i32).min(65_536),
        _ => 4096,
    };
    for fd in 3..max {
        unsafe {
            libc::close(fd);
        }
    }
}

/// The Claude Code daemon job this process runs under, if any. A session
/// the daemon hosts has `CLAUDE_JOB_DIR` set to `…/jobs/<job>`; it is the
/// one thing that ties it to the pane running `claude attach <job>`.
fn own_job() -> Option<String> {
    let dir = std::env::var("CLAUDE_JOB_DIR").ok()?;
    cmux_core::agents::job_from_dir(&dir)
}

/// Handles a Claude Code hook event: payload arrives as JSON on stdin.
/// Never fails loudly — a broken hook must not break Claude Code.
fn claude_hook(event: &str) -> Result<(), String> {
    let mut input = String::new();
    use std::io::Read;
    let _ = std::io::stdin().read_to_string(&mut input);
    let payload: serde_json::Value = serde_json::from_str(&input).unwrap_or(serde_json::json!({}));
    let tty = own_tty();
    let pane = own_pane();
    let job = own_job();

    if let Some(session_id) = payload["session_id"].as_str() {
        let _ = send_request(Request::AgentSession {
            pane_id: pane.clone(),
            tty: tty.clone(),
            agent: "claude".into(),
            session_id: session_id.to_string(),
            job: job.clone(),
            event: Some(event.to_string()),
            notification_type: payload["notification_type"].as_str().map(String::from),
            message: payload["message"].as_str().map(String::from),
        });
    }

    let notify = match event {
        "notification" => Some(
            payload["message"]
                .as_str()
                .unwrap_or("needs your attention")
                .to_string(),
        ),
        "stop" => Some("finished responding".to_string()),
        // An API error ended the turn: it stopped as surely as with `stop`,
        // and the human needs to know it did not finish.
        "stop-failure" => Some(match payload["error"].as_str() {
            Some(error) => format!("stopped on an error: {error}"),
            None => "stopped on an error".to_string(),
        }),
        _ => None, // session-start etc: session capture only
    };
    if let Some(body) = notify {
        let _ = send_request(Request::Notify {
            pane_id: pane,
            tty,
            title: Some("Claude Code".into()),
            body,
            job,
        });
    }
    Ok(())
}

/// This binary's real path, through any symlink — `~/.local/bin/mira`
/// resolves to the app's own copy, which is what a hook should call.
fn own_path() -> Result<std::path::PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(std::fs::canonicalize(&exe).unwrap_or(exe))
}

/// Idempotently installs (or removes) the Mirador hooks in
/// ~/.claude/settings.json and the `/mira-diff` skill beside them.
fn hooks(action: &str) -> Result<(), String> {
    use cmux_core::claude_integration::{self, Action};
    let action = match action {
        "setup" => Action::Setup,
        "remove" => Action::Remove,
        other => return Err(format!("unknown hooks action `{other}` (setup|remove)")),
    };
    for line in claude_integration::hooks(action, &own_path()?)? {
        println!("{line}");
    }
    if action == Action::Setup {
        println!("note: agents and /mira-diff call `mira` by name; `mira install` puts it on PATH");
    }
    Ok(())
}

/// Puts `mira` on PATH (see `claude_integration::install_on_path`).
fn install() -> Result<(), String> {
    for line in cmux_core::claude_integration::install_on_path(&own_path()?)? {
        println!("{line}");
    }
    Ok(())
}

/// The Claude Code integration, minus the parts that need a running app.
/// `hooks setup` edits a real user's settings.json, and pane targeting is
/// what makes five parallel agents notify five different tabs — both worth
/// asserting on Windows, where the paths and the environment differ.
#[cfg(test)]
mod tests {
    use super::*;

    /// How a hook knows which pane its agent runs in. The tty fallback is
    /// unix-only, so on Windows this environment variable is the only
    /// mechanism — if it stopped being read, every notification would land
    /// on whichever tab happened to be focused.
    #[test]
    fn own_pane_comes_from_the_pane_environment() {
        std::env::set_var(cmux_core::pty::PANE_ENV, "pane-abc-123");
        assert_eq!(own_pane(), Some("pane-abc-123".to_string()));

        // An empty value is not a pane; it must not be sent as one.
        std::env::set_var(cmux_core::pty::PANE_ENV, "");
        assert_eq!(own_pane(), None);

        std::env::remove_var(cmux_core::pty::PANE_ENV);
        assert_eq!(own_pane(), None);
    }
}
