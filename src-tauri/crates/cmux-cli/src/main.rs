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
    /// Install or remove the Claude Code hooks that light up Mirador tabs.
    Hooks {
        /// "setup" or "remove"
        action: String,
    },
    /// Put `mira` on your PATH: a symlink in ~/.local/bin on macOS and
    /// Linux; on Windows, the install directory joins your user PATH.
    Install,
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
            }
        }
        Command::ClaudeHook { event } => return claude_hook(&event),
        Command::Hooks { action } => return hooks(&action),
        Command::Install => return install(),
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
        other => println!("{}", serde_json::to_string_pretty(other).unwrap()),
    }
    Ok(())
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

/// Handles a Claude Code hook event: payload arrives as JSON on stdin.
/// Never fails loudly — a broken hook must not break Claude Code.
fn claude_hook(event: &str) -> Result<(), String> {
    let mut input = String::new();
    use std::io::Read;
    let _ = std::io::stdin().read_to_string(&mut input);
    let payload: serde_json::Value = serde_json::from_str(&input).unwrap_or(serde_json::json!({}));
    let tty = own_tty();
    let pane = own_pane();

    if let Some(session_id) = payload["session_id"].as_str() {
        let _ = send_request(Request::AgentSession {
            pane_id: pane.clone(),
            tty: tty.clone(),
            agent: "claude".into(),
            session_id: session_id.to_string(),
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
        _ => None, // session-start etc: session capture only
    };
    if let Some(body) = notify {
        let _ = send_request(Request::Notify {
            pane_id: pane,
            tty,
            title: Some("Claude Code".into()),
            body,
        });
    }
    Ok(())
}

const HOOK_EVENTS: &[(&str, &str)] = &[
    ("Notification", "notification"),
    ("Stop", "stop"),
    ("SessionStart", "session-start"),
];

/// True if a hook matcher entry is one we installed. Matches both the
/// current `mira claude-hook` and the legacy `cmux claude-hook` command so
/// setup can migrate and remove can clean up either.
fn is_our_hook(matcher: &serde_json::Value) -> bool {
    matcher["hooks"]
        .as_array()
        .map(|hs| {
            hs.iter()
                .any(|h| h["command"].as_str().unwrap_or("").contains("claude-hook"))
        })
        .unwrap_or(false)
}

/// Idempotently installs (or removes) Mirador hooks in ~/.claude/settings.json.
fn hooks(action: &str) -> Result<(), String> {
    let home = cmux_core::config::home_dir().ok_or("could not find your home directory")?;
    hooks_at(&std::path::PathBuf::from(home).join(".claude/settings.json"), action)
}

/// The same, against an explicit settings file — so the edit can be tested
/// without a test writing into the developer's own Claude Code config.
fn hooks_at(path: &std::path::Path, action: &str) -> Result<(), String> {
    let mut settings: serde_json::Value = match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?,
        Err(_) => serde_json::json!({}),
    };
    let hooks_obj = settings
        .as_object_mut()
        .ok_or("settings.json is not an object")?
        .entry("hooks")
        .or_insert(serde_json::json!({}));

    match action {
        "setup" => {
            for (hook_event, cli_event) in HOOK_EVENTS {
                let command = format!("mira claude-hook {cli_event}");
                let entries = hooks_obj
                    .as_object_mut()
                    .ok_or("hooks is not an object")?
                    .entry(*hook_event)
                    .or_insert(serde_json::json!([]));
                let list = entries.as_array_mut().ok_or("hook entry is not an array")?;
                // Drop any prior hook of ours (matches the old `cmux
                // claude-hook` too), then add the fresh one: idempotent and
                // migrates an earlier install to the new command name.
                list.retain(|matcher| !is_our_hook(matcher));
                list.push(serde_json::json!({
                    "hooks": [{ "type": "command", "command": command }]
                }));
                println!("installed {hook_event} hook");
            }
        }
        "remove" => {
            if let Some(obj) = hooks_obj.as_object_mut() {
                for (hook_event, _) in HOOK_EVENTS {
                    if let Some(list) = obj.get_mut(*hook_event).and_then(|v| v.as_array_mut()) {
                        list.retain(|matcher| !is_our_hook(matcher));
                    }
                }
                println!("removed Mirador hooks");
            }
        }
        other => return Err(format!("unknown hooks action `{other}` (setup|remove)")),
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!("updated {}", path.display());
    println!("note: make sure `mira` is on PATH for Claude Code (mira install)");
    Ok(())
}

/// Puts `mira` on PATH: a symlink in ~/.local/bin on unix, and on Windows
/// the install directory itself joins the user's PATH — so `mira` keeps
/// pointing at the installed app after an upgrade, and no copy goes stale.
#[cfg(unix)]
fn install() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let home = cmux_core::config::home_dir().ok_or("could not find your home directory")?;
    let bin_dir = std::path::PathBuf::from(home).join(".local/bin");
    std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;
    let target = bin_dir.join("mira");
    let _ = std::fs::remove_file(&target);
    std::os::unix::fs::symlink(&exe, &target).map_err(|e| e.to_string())?;
    println!("installed: {} -> {}", target.display(), exe.display());
    println!("make sure ~/.local/bin is on your PATH");
    Ok(())
}

#[cfg(windows)]
fn install() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe
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
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("could not update PATH: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "could not update PATH: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    if String::from_utf8_lossy(&output.stdout).trim() == "already" {
        println!("{dir} is already on your PATH");
    } else {
        println!("added to your user PATH: {dir}");
        println!("open a new terminal (or sign out and back in) to pick it up");
    }
    println!("mira: {}", exe.display());
    Ok(())
}

/// The Claude Code integration, minus the parts that need a running app.
/// `hooks setup` edits a real user's settings.json, and pane targeting is
/// what makes five parallel agents notify five different tabs — both worth
/// asserting on Windows, where the paths and the environment differ.
#[cfg(test)]
mod tests {
    use super::*;

    fn temp_settings(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mira-hooks-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    fn read(path: &std::path::Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn setup_installs_one_hook_per_event() {
        let path = temp_settings("setup");
        hooks_at(&path, "setup").unwrap();

        let settings = read(&path);
        for (hook_event, cli_event) in HOOK_EVENTS {
            let entries = settings["hooks"][hook_event].as_array().unwrap();
            assert_eq!(entries.len(), 1, "{hook_event} should have exactly one hook");
            assert_eq!(
                entries[0]["hooks"][0]["command"],
                serde_json::json!(format!("mira claude-hook {cli_event}"))
            );
        }
    }

    #[test]
    fn setup_is_idempotent_and_leaves_other_hooks_alone() {
        let path = temp_settings("idempotent");
        std::fs::write(
            &path,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"someone-elses-tool"}]}]}}"#,
        )
        .unwrap();

        hooks_at(&path, "setup").unwrap();
        hooks_at(&path, "setup").unwrap();

        let settings = read(&path);
        let stop = settings["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2, "running setup twice must not duplicate ours");
        assert!(
            stop.iter().any(|m| m["hooks"][0]["command"] == "someone-elses-tool"),
            "another tool's hook must survive"
        );
    }

    #[test]
    fn remove_takes_only_our_hooks_back_out() {
        let path = temp_settings("remove");
        std::fs::write(
            &path,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"someone-elses-tool"}]}]}}"#,
        )
        .unwrap();

        hooks_at(&path, "setup").unwrap();
        hooks_at(&path, "remove").unwrap();

        let settings = read(&path);
        let stop = settings["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 1);
        assert_eq!(stop[0]["hooks"][0]["command"], "someone-elses-tool");
    }

    #[test]
    fn a_legacy_cmux_hook_is_migrated_not_duplicated() {
        let path = temp_settings("legacy");
        std::fs::write(
            &path,
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"cmux claude-hook stop"}]}]}}"#,
        )
        .unwrap();

        hooks_at(&path, "setup").unwrap();

        let settings = read(&path);
        let stop = settings["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 1, "the old cmux hook should be replaced");
        assert_eq!(stop[0]["hooks"][0]["command"], "mira claude-hook stop");
    }

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
