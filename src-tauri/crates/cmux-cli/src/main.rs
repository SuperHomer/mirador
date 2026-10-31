//! `mira` CLI — drives the running app over its automation socket.
//! `notify` also works without the socket (prints an OSC 777 escape, so it
//! reaches Mirador through any nesting, including SSH).

use std::io::Write;

use clap::{Parser, Subcommand};
use cmux_protocol::{Request, ResponseEnvelope, SplitDir, View};

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
    /// Close a tab and every pane in it.
    CloseTab { tab: String },
    /// Focus a pane (activates its tab).
    Focus { pane: String },
    /// Show a pane alone, filling its tab (activates it); again, or with
    /// --off, shows the whole tab. PANE defaults to the focused pane.
    Zoom {
        pane: Option<String>,
        #[arg(long)]
        off: bool,
    },
    /// Show the workspace as `tabs` or as the `board`; without either,
    /// print which one is shown.
    View { view: Option<String> },
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
    /// Show the settings pane, mirador.json's settings, or open it: where
    /// `editorOpensIn` says, or as --tab or --split say.
    Settings {
        /// Open in a new tab.
        #[arg(long, conflicts_with = "split")]
        tab: bool,
        /// Open as a split of this pane.
        #[arg(long)]
        split: bool,
    },
    /// Agents: start a Claude Code one with a role; list and watch every
    /// agent, Codex, OpenCode and the rest included.
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },
    /// Teams: roles started together in a tab of their own, each told who
    /// its teammates are, and kept track of — list them, add a member,
    /// restart the ones that exited.
    Team {
        #[command(subcommand)]
        action: TeamAction,
    },
    /// Tasks: what agents were given and where each stands, as a board.
    Task {
        #[command(subcommand)]
        action: TaskAction,
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
enum TeamAction {
    /// Start a team (`.mirador/teams/<team>.md`, or yours) in a tab of its
    /// own. TASK goes to its lead as its first message.
    Start {
        team: String,
        /// The title of the task's card, instead of its first line.
        #[arg(long)]
        title: Option<String>,
        #[arg(trailing_var_arg = true)]
        task: Vec<String>,
    },
    /// Running teams and where each member is: running, exited, or closed.
    /// With --defined, the teams this directory can start instead.
    List {
        #[arg(long)]
        defined: bool,
    },
    /// Add a member to this tab's team, beside this pane.
    Add {
        role: String,
        /// What to call it; the role's name, numbered if taken, otherwise.
        #[arg(long)]
        name: Option<String>,
    },
    /// Start again this tab's members whose agent exited or whose pane was
    /// closed — the ones named, or all of them. A member's shell must be
    /// idle: a command is typed into it.
    Restart { members: Vec<String> },
    /// Start every running member of this tab's team on a new
    /// conversation, as `agent clear` does one — the lead last, so a lead
    /// running this is cleared once its turn ends.
    Clear {
        /// Answer a prompt a member is waiting on anyway.
        #[arg(long)]
        force: bool,
    },
    /// Save this tab's team — each member's conversation — as NAME (the
    /// team's name by default), to resume after the computer was turned
    /// off. Nothing is saved unless every member can be.
    Save { name: Option<String> },
    /// Open a team save in a tab of its own, each member back in its
    /// conversation.
    Resume { name: String },
    /// Every save, team's and agent's, newest first.
    Saves,
    /// What a team this directory sees tells its members, in estimated
    /// tokens, and the least that costs each turn: a lower bound, with
    /// what it leaves out.
    Cost { team: String },
}

#[derive(Subcommand)]
enum TaskAction {
    /// The board of this tab — its team's, in a team's tab — by column:
    /// each card's agent, age and title. Every tab's with --all.
    List {
        #[arg(long)]
        all: bool,
    },
    /// Write a card in this tab, in To do, and print its id. It moves only
    /// when moved: `task move`, `task done`.
    Add {
        // Not trailing_var_arg: `--to` after the title is an option, not
        // more title.
        #[arg(required = true)]
        title: Vec<String>,
        /// The agent it is for: a teammate's name in this tab, as
        /// `agent send` takes it.
        #[arg(long)]
        to: Option<String>,
        /// More about it than the title says.
        #[arg(long)]
        description: Option<String>,
    },
    /// Put a card written with `task add` in a column: todo, doing,
    /// needs-you or done. ID may be the start of one card's id. A card
    /// Mirador made follows its agent and is not moved by hand.
    Move { id: String, column: String },
    /// Put a card written with `task add` in Done.
    Done { id: String },
}

#[derive(Subcommand)]
enum AgentAction {
    /// Start an agent in a pane beside this one: Claude Code, or a role's
    /// harness, model and prompt (its own, then its way of working's).
    /// TASK becomes its first message.
    New {
        /// A role this directory sees (`mira agent roles` lists them).
        #[arg(short, long)]
        role: Option<String>,
        /// Without a role: start Codex instead of Claude Code.
        #[arg(long, value_parser = ["claude", "codex"], conflicts_with = "role")]
        harness: Option<String>,
        /// Open a new tab instead of splitting this pane.
        #[arg(long)]
        tab: bool,
        /// The title of the task's card, instead of its first line.
        #[arg(long)]
        title: Option<String>,
        #[arg(trailing_var_arg = true)]
        task: Vec<String>,
    },
    /// Every agent pane: harness, role, model, and what it is doing.
    List,
    /// A Claude Code agent's task progress and its subagents (this pane's
    /// agent, or the focused pane's).
    Activity {
        /// The agent's pane, instead of this one.
        #[arg(long)]
        pane: Option<String>,
    },
    /// Type a message into another agent's terminal, as its next prompt.
    /// TO is a teammate's name in this tab — a team member's, a session's,
    /// a role's — or a pane id. It says who it is from when this pane is
    /// an agent. Refused for a pane running no agent, and for an agent
    /// waiting on a person (a permission prompt) unless --force.
    Send {
        to: String,
        // Not trailing_var_arg: `--card`, `--wait` after the message are
        // options, not more message.
        #[arg(required = true)]
        message: Vec<String>,
        /// Answer a prompt the agent is waiting on anyway.
        #[arg(long)]
        force: bool,
        /// The card the message is for, written with `task add` (its id,
        /// or the start of it): logged on that card, not on a new one.
        #[arg(long)]
        card: Option<String>,
        /// The title of the message's card, instead of its first line.
        #[arg(long, conflicts_with = "card")]
        title: Option<String>,
        /// Then wait for it to finish, as `agent wait` does.
        #[arg(long)]
        wait: bool,
        /// With --wait: give up after this many seconds.
        #[arg(long, default_value_t = 1800)]
        timeout: u64,
    },
    /// Start an agent on a new conversation, as typing /clear into it does
    /// (Codex's /new): NAME as `agent send` takes it, else this pane's
    /// agent. Its role's prompt is kept. A working agent starts over once
    /// its turn ends; one waiting on a person is refused unless --force.
    Clear {
        name: Option<String>,
        /// The agent's pane, instead of a name.
        #[arg(long, conflicts_with = "name")]
        pane: Option<String>,
        /// Answer a prompt the agent is waiting on anyway.
        #[arg(long)]
        force: bool,
    },
    /// Save an agent's conversation to resume after a restart: NAME as
    /// `agent send` takes it, else this pane's agent. Saved as what it
    /// goes by, or --as.
    Save {
        name: Option<String>,
        /// The agent's pane, instead of a name.
        #[arg(long, conflicts_with = "name")]
        pane: Option<String>,
        /// The save's name.
        #[arg(long = "as")]
        save_as: Option<String>,
    },
    /// Open one agent's save beside this pane, back in its conversation.
    Resume {
        name: String,
        /// Open a new tab instead of splitting this pane.
        #[arg(long)]
        tab: bool,
    },
    /// Wait until an agent has worked and stopped: exit 0 when it is idle,
    /// 2 when it stopped to ask a person, 3 when it exited, 124 on timeout.
    /// Needs Claude Code's hooks (`mira hooks setup`) to see it stop.
    Wait {
        name: String,
        #[arg(long, default_value_t = 1800)]
        timeout: u64,
    },
    /// Open the editor for the role, way of working and team files this
    /// directory sees — the repository's `.mirador`, and yours — beside
    /// this pane.
    Edit {
        /// Open a new tab instead of splitting this pane.
        #[arg(long)]
        tab: bool,
    },
    /// The roles `agent new --role` accepts here — this repository's
    /// `.mirador/roles`, then yours — and the ways of working they follow.
    Roles,
    /// The tasks given in this tab and how each turn ended, oldest first:
    /// of the agent NAME (a teammate's name, or a pane id) when given, of
    /// every tab with --all.
    History {
        name: Option<String>,
        /// The agent's pane, instead of a name.
        #[arg(long, conflicts_with = "name")]
        pane: Option<String>,
        #[arg(long, conflicts_with_all = ["name", "pane"])]
        all: bool,
        /// One card's history instead: its id, or the start of it.
        #[arg(long, conflicts_with_all = ["name", "pane", "all"])]
        card: Option<String>,
    },
    /// What an agent did, turn by turn, read from its own transcript
    /// (Claude Code) or session file (Codex): each turn's prompt, then a
    /// line per action — when, how long, ✓ or ✗, the tool and what it was
    /// about. Of the agent NAME (a teammate's name, or a pane id), else
    /// of this pane.
    Trail {
        name: Option<String>,
        /// The agent's pane, instead of a name.
        #[arg(long, conflicts_with = "name")]
        pane: Option<String>,
        /// How many of its latest turns.
        #[arg(long, default_value_t = 3)]
        turns: u32,
        /// One action's full input and output instead, by the id shown
        /// at the end of its line.
        #[arg(long)]
        detail: Option<String>,
    },
    /// Show this tab as the agent wall on its Tasks side, the task board:
    /// its agents' cards in To do, Doing, Needs you and Done, and every
    /// tab's at a switch.
    Board {
        /// Ignored. The board was a pane of its own, which this opened in
        /// a new tab, until it became the wall's Tasks side; scripts
        /// written for v0.1.33 still pass it.
        #[arg(long, hide = true)]
        tab: bool,
    },
    /// Show this tab as the agent wall, its agents' terminals in place of
    /// its splits; again to show the splits. With --focus or --grid, show
    /// the wall in that layout (turning it on, never off).
    Wall {
        /// One agent large, the others down a rail.
        #[arg(long, conflicts_with = "grid")]
        focus: bool,
        /// Every agent at full size, a page at a time.
        #[arg(long)]
        grid: bool,
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
    /// A PNG of the page, written to a file whose path it prints: the
    /// part of the page the pane shows, at screen resolution — what a
    /// person would see there, even while the pane is hidden. Without
    /// --out, a new file in the temp directory.
    Screenshot {
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        #[arg(long)]
        pane: Option<String>,
    },
    /// Click an element by snapshot id or CSS selector.
    Click {
        target: String,
        #[arg(long)]
        pane: Option<String>,
    },
    /// Fill an input (snapshot id or CSS selector) with a value: sets it at
    /// once, with one input and change event. To type it key by key, for
    /// pages that listen to keys, use `type`.
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
    /// Go back in the browser pane's history.
    Back {
        #[arg(long)]
        pane: Option<String>,
    },
    /// Go forward in the browser pane's history.
    Forward {
        #[arg(long)]
        pane: Option<String>,
    },
    /// Reload the browser pane's page (its console log starts empty).
    Reload {
        #[arg(long)]
        pane: Option<String>,
    },
    /// The visible text of an element (a CSS selector), or of the whole
    /// page: what a person reads there, as the browser lays it out. Long
    /// text stops at 100,000 characters, saying how many more there were.
    Text {
        selector: Option<String>,
        #[arg(long)]
        pane: Option<String>,
    },
    /// Type text into an element key by key (keydown, keypress, input,
    /// keyup per character) — for pages that react to keys: autocomplete,
    /// validation on keyup. It types at the cursor, replacing a selection,
    /// as a person would; a field or editable element not already focused
    /// gets the cursor at the end of its text, so typing there appends. `fill` sets a value
    /// at once. No change event until the field loses focus (`press Tab`),
    /// as in a browser. The events are synthetic (isTrusted is false),
    /// which a few pages ignore.
    Type {
        target: String,
        text: String,
        #[arg(long)]
        pane: Option<String>,
    },
    /// Press a key or chord on the focused element, or on --target (focused
    /// first, the cursor at the end of a field's or editable's text): Enter,
    /// Tab, Escape, Backspace, Delete, Space, arrows, Home, End, PageUp,
    /// PageDown, a letter or digit, with Shift+, Control+, Alt+, Meta+
    /// ("Shift+Tab", "Meta+A"). Enter in a form's field submits the form,
    /// as a browser does. Synthetic events (isTrusted is false).
    Press {
        key: String,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        pane: Option<String>,
    },
    /// The page's console output and errors (uncaught, unhandled
    /// rejections, failed loads), oldest first. Per page: a navigation
    /// starts empty. Exits 0 whatever it shows — it only reads.
    Console {
        /// Only errors.
        #[arg(long)]
        errors: bool,
        /// Empty the log after printing it.
        #[arg(long)]
        clear: bool,
        #[arg(long)]
        pane: Option<String>,
    },
    /// Wait until the page shows a CSS selector, some text, a URL, or no
    /// longer shows a selector — instead of sleeping.
    #[command(group(clap::ArgGroup::new("until").required(true).args(["selector", "text", "url", "gone"])))]
    Wait {
        /// Until an element matching this CSS selector is shown.
        selector: Option<String>,
        /// Until the page's visible text contains this.
        #[arg(long)]
        text: Option<String>,
        /// Until the loaded page's URL contains this.
        #[arg(long)]
        url: Option<String>,
        /// Until nothing matching this CSS selector is shown (a spinner).
        #[arg(long, value_name = "SELECTOR")]
        gone: Option<String>,
        /// Seconds before giving up.
        #[arg(long, default_value_t = 10)]
        timeout: u64,
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
    // What to say when a command succeeds with nothing to show, where
    // silence could read as "did nothing": `wait` met, an empty console.
    let mut empty_note: Option<String> = None;
    let req = match cli.command {
        Command::ListTabs => Request::ListTabs,
        Command::NewTab { command } => Request::NewTab { command },
        Command::Split { dir, pane, command } => Request::SplitPane {
            pane_id: pane,
            dir: parse_dir(&dir)?,
            command,
        },
        Command::ClosePane { pane } => Request::ClosePane { pane_id: pane },
        Command::CloseTab { tab } => Request::CloseTab { tab_id: tab },
        Command::Focus { pane } => Request::FocusPane { pane_id: pane },
        Command::Zoom { pane, off } => Request::ZoomPane {
            pane_id: pane,
            zoom: off.then_some(false),
        },
        Command::View { view } => Request::View {
            view: view.as_deref().map(parse_view).transpose()?,
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
        Command::Settings { tab, split } => Request::Settings {
            pane_id: own_pane(),
            target: if tab {
                Some("tab".to_string())
            } else {
                split.then(|| "split".to_string())
            },
        },
        Command::Agent { action } => match action {
            AgentAction::New { role, harness, tab, title, task } => Request::AgentNew {
                role,
                title,
                harness: harness.map(|h| match h.as_str() {
                    "codex" => cmux_protocol::Harness::Codex,
                    _ => cmux_protocol::Harness::Claude,
                }),
                task: (!task.is_empty()).then(|| task.join(" ")),
                target: Some(if tab { "tab" } else { "split" }.to_string()),
                pane_id: own_pane(),
            },
            AgentAction::List => Request::AgentList,
            AgentAction::Activity { pane } => Request::AgentActivity {
                pane_id: pane.or_else(own_pane),
            },
            AgentAction::Roles => return agent_roles(cli.json),
            AgentAction::Edit { tab } => Request::AgentEditor {
                pane_id: own_pane(),
                target: tab.then(|| "tab".to_string()),
            },
            AgentAction::Send { to, message, force, card, title, wait, timeout } => {
                let started = now_ms();
                let resp = send_request(Request::AgentSend {
                    to: to.clone(),
                    message: message.join(" "),
                    pane_id: own_pane(),
                    force,
                    card,
                    title,
                })?;
                if !wait || !resp.ok {
                    return render(resp, cli.json, false, None);
                }
                let pane = resp.data.as_ref().and_then(|d| d["paneId"].as_str()).unwrap_or_default().to_string();
                return agent_wait(&to, &pane, started, timeout);
            }
            AgentAction::Clear { name, pane, force } => Request::AgentClear {
                name: name.or(pane),
                pane_id: own_pane(),
                force,
            },
            AgentAction::Save { name, pane, save_as } => Request::AgentSave {
                name: name.or(pane),
                save_as,
                pane_id: own_pane(),
            },
            AgentAction::Resume { name, tab } => Request::AgentResume { name, pane_id: own_pane(), tab },
            AgentAction::Wait { name, timeout } => {
                let started = now_ms();
                let resp = send_request(Request::AgentFind { name: name.clone(), pane_id: own_pane() })?;
                if !resp.ok {
                    return render(resp, cli.json, false, None);
                }
                let pane = resp.data.as_ref().and_then(|d| d["paneId"].as_str()).unwrap_or_default().to_string();
                return agent_wait(&name, &pane, started, timeout);
            }
            AgentAction::History { name, pane, all, card } => Request::AgentHistory {
                name: name.or(pane),
                all,
                pane_id: own_pane(),
                card,
            },
            AgentAction::Trail { name, pane, turns, detail } => match detail {
                Some(action_id) => Request::AgentTrailDetail { action_id, name: name.or(pane), pane_id: own_pane() },
                None => Request::AgentTrail { name: name.or(pane), pane_id: own_pane(), turns: Some(turns) },
            },
            AgentAction::Board { tab: _ } => {
                let resp = send_request(Request::AgentBoard { pane_id: own_pane() })?;
                if cli.json || !resp.ok {
                    return render(resp, cli.json, false, None);
                }
                println!("wall on: tasks");
                return Ok(());
            }
            AgentAction::Wall { focus, grid } => {
                let mode = if grid {
                    Some(cmux_protocol::WallMode::Grid)
                } else {
                    focus.then_some(cmux_protocol::WallMode::Focus)
                };
                let resp = send_request(Request::AgentWall { pane_id: own_pane(), mode })?;
                if cli.json || !resp.ok {
                    return render(resp, cli.json, false, None);
                }
                let on = resp.data.as_ref().and_then(|d| d["wall"].as_bool()).unwrap_or(false);
                match mode {
                    Some(cmux_protocol::WallMode::Grid) => println!("wall on: grid"),
                    Some(cmux_protocol::WallMode::Focus) => println!("wall on: focus"),
                    None => println!("wall {}", if on { "on" } else { "off" }),
                }
                return Ok(());
            }
        },
        Command::Team { action } => match action {
            TeamAction::Start { team, title, task } => Request::TeamStart {
                team,
                task: (!task.is_empty()).then(|| task.join(" ")),
                pane_id: own_pane(),
                title,
            },
            TeamAction::List { defined: true } => return defined_teams(cli.json),
            TeamAction::List { defined: false } => Request::TeamList,
            TeamAction::Add { role, name } => Request::TeamAdd { role, name, pane_id: own_pane() },
            TeamAction::Restart { members } => Request::TeamRestart { members, pane_id: own_pane() },
            TeamAction::Clear { force } => Request::TeamClear { pane_id: own_pane(), force },
            TeamAction::Save { name } => Request::TeamSave { name, pane_id: own_pane() },
            TeamAction::Resume { name } => Request::TeamResume { name },
            TeamAction::Saves => Request::SaveList,
            TeamAction::Cost { team } => {
                let resp = send_request(Request::TeamCost { team, pane_id: own_pane() })?;
                if cli.json || !resp.ok {
                    return render(resp, cli.json, false, None);
                }
                let weight = resp.data.and_then(|d| serde_json::from_value(d["weight"].clone()).ok());
                return weight.map(|w| render_team_weight(&w)).ok_or_else(|| "the app sent no team weight".to_string());
            }
        },
        Command::Task { action } => match action {
            TaskAction::List { all } => Request::TaskList { all, pane_id: own_pane() },
            TaskAction::Add { title, to, description } => {
                Request::TaskAdd { title: title.join(" "), to, pane_id: own_pane(), description }
            }
            TaskAction::Move { id, column } => Request::TaskMove {
                card: id,
                column: cmux_core::tasks::parse_column(&column)?,
                pane_id: own_pane(),
            },
            TaskAction::Done { id } => Request::TaskMove {
                card: id,
                column: cmux_protocol::TaskColumn::Done,
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
            BrowserAction::Screenshot { out, pane } => return browser_screenshot(pane, out, cli.json),
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
            BrowserAction::Text { selector, pane } => Request::BrowserText {
                pane_id: pane,
                selector,
            },
            BrowserAction::Type { target, text, pane } => Request::BrowserType {
                pane_id: pane,
                target,
                text,
            },
            BrowserAction::Press { key, target, pane } => Request::BrowserPress {
                pane_id: pane,
                key,
                target,
            },
            BrowserAction::Console { errors, clear, pane } => {
                empty_note = Some(if errors { "(no errors)" } else { "(no console output)" }.into());
                Request::BrowserConsole {
                    pane_id: pane,
                    errors,
                    clear,
                }
            }
            BrowserAction::Wait {
                selector,
                text,
                url,
                gone,
                timeout,
                pane,
            } => {
                empty_note = Some(match (&selector, &text, &url, &gone) {
                    (Some(s), ..) => format!("{s} is shown"),
                    (_, Some(t), ..) => format!("text {t:?} found"),
                    (_, _, Some(u), _) => format!("url contains {u}"),
                    (.., Some(g)) => format!("{g} is gone"),
                    _ => String::new(),
                });
                Request::BrowserWait {
                    pane_id: pane,
                    selector,
                    text,
                    url,
                    gone,
                    timeout_secs: Some(timeout),
                }
            }
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
    render(response, cli.json, quiet_output, empty_note.as_deref())
}

/// The app sends the PNG; the CLI writes it, so the app writes nowhere.
fn browser_screenshot(pane: Option<String>, out: Option<std::path::PathBuf>, json: bool) -> Result<(), String> {
    use base64::Engine;
    let resp = send_request(Request::BrowserScreenshot { pane_id: pane })?;
    if !resp.ok {
        return render(resp, json, false, None);
    }
    let data = resp.data.unwrap_or_default();
    let pane = data["paneId"].as_str().unwrap_or("browser");
    let png = base64::engine::general_purpose::STANDARD
        .decode(data["png"].as_str().unwrap_or_default())
        .map_err(|e| format!("bad screenshot from the app: {e}"))?;
    let path = match out {
        // Named by the caller, so theirs to overwrite.
        Some(path) => {
            std::fs::write(&path, png).map_err(|e| format!("{}: {e}", path.display()))?;
            path
        }
        None => {
            let dir = std::env::temp_dir();
            let (path, mut file) = new_file(&dir, &format!("mira-screenshot-{pane}-{}", now_ms()), "png")
                .map_err(|e| format!("{}: {e}", dir.display()))?;
            file.write_all(&png).map_err(|e| format!("{}: {e}", path.display()))?;
            path
        }
    };
    if json {
        println!("{}", serde_json::json!({ "paneId": pane, "path": path }));
    } else {
        println!("{}", path.display());
    }
    Ok(())
}

/// A file in `dir` that did not exist before: `<stem>.<ext>`, else
/// `<stem>-2.<ext>`, `-3`… Two screenshots of a pane in the same
/// millisecond would otherwise share a path, one caller reading the
/// other's image.
fn new_file(dir: &std::path::Path, stem: &str, ext: &str) -> std::io::Result<(std::path::PathBuf, std::fs::File)> {
    for n in 1.. {
        let name = if n == 1 { format!("{stem}.{ext}") } else { format!("{stem}-{n}.{ext}") };
        let path = dir.join(name);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!("a free name")
}

fn parse_dir(s: &str) -> Result<SplitDir, String> {
    match s {
        "row" | "right" | "horizontal" => Ok(SplitDir::Row),
        "column" | "down" | "vertical" => Ok(SplitDir::Column),
        other => Err(format!("invalid direction `{other}` (row|column)")),
    }
}

fn parse_view(s: &str) -> Result<View, String> {
    match s {
        "tabs" => Ok(View::Tabs),
        "board" => Ok(View::Board),
        other => Err(format!("invalid view `{other}` (tabs|board)")),
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

fn render(resp: ResponseEnvelope, raw_json: bool, quiet: bool, empty_note: Option<&str>) -> Result<(), String> {
    if !resp.ok {
        return Err(resp.error.unwrap_or_else(|| "unknown error".into()));
    }
    let data = resp.data.unwrap_or(serde_json::Value::Null);
    if raw_json {
        println!("{}", serde_json::to_string_pretty(&data).unwrap());
        return Ok(());
    }
    match &data {
        serde_json::Value::Null => {
            if let Some(note) = empty_note {
                println!("{note}");
            }
        }
        serde_json::Value::Object(map) => {
            // Something to know about a command that still did its job: a
            // navigation whose page had not loaded in time.
            if let Some(note) = map.get("note").and_then(|v| v.as_str()) {
                eprintln!("mira: {note}");
            }
            if map.contains_key("note") && map.len() == 1 {
                // Only the note, said above.
            } else if map.contains_key("output") {
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
            } else if let Some(entries) = map.get("console").and_then(|v| v.as_array()) {
                let dropped = map["dropped"].as_u64().unwrap_or(0);
                if entries.is_empty() && dropped == 0 {
                    if let Some(note) = empty_note {
                        println!("{note}");
                    }
                }
                render_console(entries, dropped);
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
            } else if map.contains_key("subagents") {
                render_activity(&data);
            } else if let Some(saves) = map.get("saves") {
                render_saves(serde_json::from_value(saves.clone()).map_err(|e| format!("bad response: {e}"))?);
            } else if let Some(saved) = map.get("checkpoint") {
                let saved: cmux_protocol::Checkpoint =
                    serde_json::from_value(saved.clone()).map_err(|e| format!("bad response: {e}"))?;
                let names: Vec<&str> = saved.members.iter().map(|m| m.name.as_str()).collect();
                println!("saved {}: {}", saved.name, names.join(", "));
            } else if let Some(teams) = map.get("teams").and_then(|v| v.as_array()) {
                render_teams(teams);
            } else if let Some(cards) = map.get("cards") {
                render_cards(serde_json::from_value(cards.clone()).map_err(|e| format!("bad response: {e}"))?);
            } else if let Some(trail) = map.get("trail") {
                render_trail(serde_json::from_value(trail.clone()).map_err(|e| format!("bad response: {e}"))?);
            } else if let Some(action) = map.get("action") {
                render_action(serde_json::from_value(action.clone()).map_err(|e| format!("bad response: {e}"))?);
            } else if let Some(history) = map.get("history") {
                render_history(serde_json::from_value(history.clone()).map_err(|e| format!("bad response: {e}"))?);
            } else if let Some(members) = map.get("members").and_then(|v| v.as_array()) {
                for m in members {
                    println!(
                        "{:<14}  {:<10}  {}",
                        m["name"].as_str().unwrap_or(""),
                        m["outcome"].as_str().unwrap_or(""),
                        m["pane"].as_str().unwrap_or("")
                    );
                }
            } else if let Some(view) = map
                .get("view")
                .and_then(|v| v.as_str())
                // A workspace snapshot says its view too.
                .filter(|_| !map.contains_key("tabs"))
            {
                println!("{view}");
            } else if let Some(id) = map
                .get("paneId")
                .or_else(|| map.get("tabId"))
                .or_else(|| map.get("cardId"))
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

/// Running teams: a line per team, then one per member with its state.
fn render_saves(saves: Vec<cmux_protocol::Checkpoint>) {
    if saves.is_empty() {
        println!("(no saves — `mira team save` or `mira agent save` makes one)");
        return;
    }
    let now = now_ms();
    for s in saves {
        let what = s.team.as_deref().map_or_else(|| "agent".to_string(), |t| format!("team {t}"));
        let names: Vec<&str> = s.members.iter().map(|m| m.name.as_str()).collect();
        println!(
            "{:<14}  {:<14}  {:>4} ago  {}",
            s.name,
            what,
            cmux_core::tasks::span(now.saturating_sub(s.saved_at)),
            names.join(", ")
        );
    }
}

fn render_teams(teams: &[serde_json::Value]) {
    if teams.is_empty() {
        println!("(no teams running — `mira team start <team>` starts one)");
        return;
    }
    for t in teams {
        println!("{}  (tab {})", t["name"].as_str().unwrap_or(""), t["tabId"].as_str().unwrap_or(""));
        for m in t["members"].as_array().into_iter().flatten() {
            let status = match (m["state"].as_str(), m["status"].as_str()) {
                (Some("running"), Some("needsYou")) => "needs-you",
                (Some("running"), Some(s)) => s,
                (Some(state), _) => state,
                _ => "-",
            };
            let lead = if m["lead"].as_bool() == Some(true) { "  (lead)" } else { "" };
            println!(
                "  {:<14}  {:<12}  {:<9}  {}{lead}",
                m["name"].as_str().unwrap_or(""),
                m["role"].as_str().unwrap_or(""),
                status,
                m["pane"].as_str().unwrap_or(""),
            );
        }
    }
}

/// The board: a heading per column that has cards, then a line per card
/// with its agent, how long ago it last moved, and its title.
fn render_cards(cards: Vec<cmux_protocol::TaskCard>) {
    use cmux_protocol::TaskColumn::*;
    if cards.is_empty() {
        println!("(no tasks — they appear as agents are given them: `mira agent send`, `team start`)");
        return;
    }
    let now = now_ms();
    for (column, heading) in [(Todo, "To do"), (Doing, "Doing"), (NeedsYou, "Needs you"), (Done, "Done")] {
        let mut here = cards.iter().filter(|c| c.column == column).peekable();
        if here.peek().is_none() {
            continue;
        }
        println!("{heading}");
        for c in here {
            println!(
                "  {}  {:<14}  {:>4}  {}",
                c.id,
                c.assignee.as_deref().unwrap_or("-"),
                cmux_core::tasks::span(now.saturating_sub(c.updated_ms)),
                c.title,
            );
        }
    }
}

/// A trail: each turn's prompt and how long ago it began, then a line per
/// action — how long ago, how long it took, its status, its tool, what it
/// was about, and its id for `--detail`.
fn render_trail(trail: cmux_protocol::AgentTrail) {
    use cmux_protocol::ActionStatus::*;
    if trail.turns.is_empty() {
        println!("({})", trail.reason.as_deref().unwrap_or("no actions yet"));
        return;
    }
    let now = now_ms();
    let ago = |ms: Option<u64>| ms.map_or("-".to_string(), |ms| format!("{} ago", cmux_core::tasks::span(now.saturating_sub(ms))));
    for turn in trail.turns {
        println!("{:>8}  {}", ago(turn.started_ms), turn.prompt.as_deref().unwrap_or("(no prompt)"));
        for a in turn.actions {
            let mark = match a.status {
                Ok => "✓",
                Error => "✗",
                Running => "…",
                Interrupted => "-",
            };
            let took = a.duration_ms.map_or(String::new(), took);
            let exit = a.exit_code.filter(|&c| c != 0).map_or(String::new(), |c| format!("  (exit {c})"));
            println!("  {:>8}  {took:>6}  {mark} {:<10} {}{exit}  [{}]", ago(a.started_ms), a.tool, a.summary, a.id);
        }
    }
}

/// How long an action took: milliseconds, seconds, or minutes and up.
fn took(ms: u64) -> String {
    match ms {
        0..=999 => format!("{ms}ms"),
        1000..=59_999 => format!("{:.1}s", ms as f64 / 1000.0),
        _ => cmux_core::tasks::span(ms),
    }
}

/// One action in full: its input, then its output.
fn render_action(action: cmux_protocol::ActionDetail) {
    println!("{}  {}", action.tool, action.id);
    println!("--- input");
    println!("{}", action.input);
    println!("--- output{}", if action.is_error { " (error)" } else { "" });
    println!("{}", action.output.as_deref().unwrap_or("(no result yet)"));
}

/// The log: a line per event, oldest first, with how long ago it was.
fn render_history(history: Vec<cmux_protocol::TaskHistoryEntry>) {
    if history.is_empty() {
        println!("(no tasks yet)");
        return;
    }
    let now = now_ms();
    for entry in history {
        println!("{:>4} ago  {}", cmux_core::tasks::span(now.saturating_sub(entry.event.at_ms)), entry.summary);
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Polls `pane`'s agent until it has worked and stopped since `started`,
/// then exits with what happened: 0 idle, 2 needs a person, 3 exited, 124
/// timed out.
fn agent_wait(name: &str, pane: &str, started: u64, timeout: u64) -> Result<(), String> {
    use cmux_core::handoff::{Wait, WaitEnd};
    let mut wait = Wait { started_ms: started, seen_working: false };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    loop {
        let resp = send_request(Request::AgentList)?;
        let agents = resp.data.as_ref().and_then(|d| d["agents"].as_array().cloned()).unwrap_or_default();
        let me = agents.iter().find(|a| a["paneId"].as_str() == Some(pane));
        let status = me.and_then(|a| serde_json::from_value(a["status"].clone()).ok());
        let since = me.and_then(|a| a["sinceMs"].as_u64());
        if let Some(end) = wait.observe(me.is_some(), status, since) {
            let (code, what) = match end {
                WaitEnd::Idle => (0, "finished"),
                WaitEnd::NeedsYou => (2, "is waiting on a person"),
                WaitEnd::Exited => (3, "exited"),
            };
            eprintln!("{name} {what}");
            std::process::exit(code);
        }
        if std::time::Instant::now() >= deadline {
            eprintln!("{name}: still not done after {timeout}s");
            std::process::exit(124);
        }
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
}

/// The teams this directory can start: `.mirador/teams`, then yours.
fn defined_teams(json: bool) -> Result<(), String> {
    let cwd = std::env::current_dir().ok();
    let catalog = cmux_core::roles::catalog(cwd.as_deref());
    if json {
        println!("{}", serde_json::to_string_pretty(&catalog.teams).map_err(|e| e.to_string())?);
        return Ok(());
    }
    if catalog.teams.is_empty() {
        println!("(no teams — add .mirador/teams/<name>.md to the repository)");
    }
    for t in &catalog.teams {
        let members: Vec<String> = t
            .members
            .iter()
            .map(|m| if m.count > 1 { format!("{}×{}", m.role, m.count) } else { m.role.clone() })
            .collect();
        let mut line = format!("{:<14}  {}", t.name, members.join(", "));
        if let Some(d) = &t.description {
            line.push_str(&format!("  — {d}"));
        }
        if let Some(p) = &t.problem {
            line.push_str(&format!("  (cannot start: {p})"));
        }
        println!("{line}");
    }
    Ok(())
}

/// A team's weight, a member per line — name, role, model, tokens, the
/// floor per turn — then the team's, then what it leaves out.
fn render_team_weight(w: &cmux_protocol::TeamWeight) {
    let price = |start: Option<f64>, floor: Option<f64>| match (start, floor) {
        (Some(start), Some(floor)) => format!("≈ ${start:.4} to start, ≥ ${floor:.4} a turn"),
        _ => "no price".to_string(),
    };
    for s in &w.seats {
        let model = s.model.as_deref().unwrap_or("default");
        println!(
            "{:<14}  {:<16}  {:<22}  {:>8} tokens  {}",
            s.name,
            s.role,
            model,
            s.tokens,
            price(s.start_usd, s.per_turn_floor_usd)
        );
    }
    // With none priced, "no price" says it all.
    let unpriced = match w.unpriced {
        0 => String::new(),
        n if n as usize == w.seats.len() => String::new(),
        1 => " (1 member with no price left out)".to_string(),
        n => format!(" ({n} members with no price left out)"),
    };
    println!(
        "{:<14}  {:<16}  {:<22}  {:>8} tokens  {}{unpriced}",
        w.team,
        format!("{} members", w.seats.len()),
        "",
        w.tokens,
        price(w.start_usd, w.per_turn_floor_usd)
    );
    println!("\nBy model");
    for m in &w.by_model {
        let model = m.model.as_deref().unwrap_or("default");
        // Claude Code's models name it; any other harness is said.
        let model = match m.harness {
            cmux_protocol::Harness::Claude => model.to_string(),
            h => format!("{model} ({})", format!("{h:?}").to_lowercase()),
        };
        println!(
            "  {:<28}  {:<24}  {:>8} tokens  {}",
            model,
            m.seats.join(", "),
            m.tokens,
            price(m.start_usd, m.per_turn_floor_usd)
        );
    }
    println!("\nHeaviest");
    for p in w.heaviest.iter().take(5) {
        use cmux_protocol::PromptLayerKind::*;
        let what = match (p.kind, &p.role) {
            (Role, Some(role)) => format!("role prompt: {role}"),
            (Role, None) => "role prompt".to_string(),
            (RoleWay, _) => "role's way".to_string(),
            (Team, _) => "place in the team".to_string(),
            (Charter, _) => "charter".to_string(),
            (TeamWay, _) => "team's way".to_string(),
        };
        // Copies of different sizes (Mirador's own part): their average.
        let each = p.tokens_per_copy.map_or(format!("~{}", p.tokens / p.copies.max(1) as u64), |n| n.to_string());
        println!(
            "  {:<32}  {:<34}  {:>5} × {:<2}  {:>8} tokens  {:>3.0}%",
            what,
            p.source,
            each,
            p.copies,
            p.tokens,
            p.share * 100.0
        );
    }
    println!();
    println!(
        "Tokens are Mirador's prompts to each member, estimated at 4 characters a token; \
         the first turn writes them to the prompt cache, and every turn reads them back from it \
         at least. Not counted: the harness's own \
         system prompt and tools, CLAUDE.md, MCP tool schemas, the conversation and all output. \
         Prices as of {}.",
        w.prices_checked
    );
}

/// Agents as a table, one per line: pane, status, harness, name (the
/// session's, else the role), model, and what it is blocked on when it is.
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
            "{}  {:<9}  {:<11}  {:<12}  {}",
            a["paneId"].as_str().unwrap_or(""),
            status,
            a["harness"].as_str().unwrap_or("claude"),
            a["member"].as_str().or(a["name"].as_str()).or(a["role"].as_str()).unwrap_or("-"),
            a["model"].as_str().unwrap_or("default"),
        );
        if let Some(message) = a["message"].as_str() {
            line.push_str(&format!("  — {message}"));
        }
        println!("{line}");
    }
}

/// An agent's activity: the session's line, then one per subagent with
/// its state, progress, tokens, cost and how long it ran, then what they
/// cost together.
fn render_activity(data: &serde_json::Value) {
    // Cents from a cent up, four decimals below; "-" for none priced, and
    // a "+" when some replies had no price and are not in it.
    let dollars = |usd: f64| if usd >= 0.01 { format!("${usd:.2}") } else { format!("${usd:.4}") };
    let cost = |v: &serde_json::Value| {
        let more = if v["unpriced"].as_bool() == Some(true) { "+" } else { "" };
        v["costUsd"].as_f64().map_or("-".to_string(), |usd| format!("{}{more}", dollars(usd)))
    };
    let tokens = |v: &serde_json::Value| {
        let n = v.as_u64().unwrap_or(0);
        match n {
            0..=999 => n.to_string(),
            1000..=999_999 => format!("{}k", n / 1000),
            _ => format!("{:.1}M", n as f64 / 1e6),
        }
    };
    let tasks = |s: &serde_json::Value| match s["tasks"].as_object() {
        Some(t) => format!("{}/{}", t["done"], t["total"]),
        None => "-".into(),
    };
    let s = &data["session"];
    let mut line = format!(
        "session   {:<5}  {:>6} tok  {:>8}  ctx {:>5}  {}",
        tasks(s),
        tokens(&s["tokens"]),
        cost(s),
        tokens(&s["context"]),
        s["model"].as_str().unwrap_or("-"),
    );
    if let Some(current) = s["tasks"]["current"].as_str() {
        line.push_str(&format!("  — {current}"));
    }
    println!("{line}");
    let subagents = data["subagents"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    if subagents.is_empty() {
        println!("(no subagents)");
    }
    for a in subagents {
        let secs = match (a["startedMs"].as_u64(), a["lastMs"].as_u64()) {
            (Some(start), Some(last)) => last.saturating_sub(start) / 1000,
            _ => 0,
        };
        println!(
            "{:<8}  {:<5}  {:>6} tok  {:>8}  ctx {:>5}  {:>2}:{:02}  {}  {}",
            a["state"].as_str().unwrap_or("-"),
            tasks(a),
            tokens(&a["tokens"]),
            cost(a),
            tokens(&a["context"]),
            secs / 60,
            secs % 60,
            a["agentType"].as_str().unwrap_or("agent"),
            a["description"].as_str().unwrap_or(""),
        );
    }
    let all: Vec<&serde_json::Value> = std::iter::once(s).chain(subagents).collect();
    let priced: Vec<f64> = all.iter().filter_map(|v| v["costUsd"].as_f64()).collect();
    let unpriced = all.iter().any(|v| v["unpriced"].as_bool() == Some(true));
    let spent = if priced.is_empty() { "-".to_string() } else { dollars(priced.iter().sum()) };
    println!(
        "cost      {spent}{}  at API list prices for the tokens its transcripts record — on a Claude \
         subscription, what it would have cost, not what is billed{}",
        if unpriced && !priced.is_empty() { "+" } else { "" },
        if unpriced { "; some replies had no known price and are not in it" } else { "" },
    );
}

/// `mira agent roles`: read from the config file, as the app reads it — no
/// running app needed.
fn agent_roles(json: bool) -> Result<(), String> {
    // This directory's roles: the pane's, since `mira` runs in it.
    let cwd = std::env::current_dir().ok();
    let catalog = cmux_core::roles::catalog(cwd.as_deref());
    if json {
        println!("{}", serde_json::to_string_pretty(&catalog).map_err(|e| e.to_string())?);
        return Ok(());
    }
    let source = |s: cmux_protocol::RoleSource| match s {
        cmux_protocol::RoleSource::Repo => "repo",
        cmux_protocol::RoleSource::Global => "yours",
        cmux_protocol::RoleSource::Config => "mirador.json",
        cmux_protocol::RoleSource::Builtin => "built-in",
    };
    if catalog.roles.is_empty() {
        println!(
            "(no roles — add .mirador/roles/<name>.md to the repository, or {})",
            cmux_core::config::config_dir().join("roles").join("<name>.md").display()
        );
    }
    for r in &catalog.roles {
        let mut model = r.model.clone().unwrap_or_else(|| "default model".into());
        if let Some(effort) = &r.effort {
            model.push_str(&format!(" · {effort}"));
        }
        let mut line = format!("{:<14}  {:<12}  {model:<21}  {:<12}", r.name, r.harness.label(), source(r.source));
        if let Some(profile) = &r.profile {
            line.push_str(&format!("  profile: {profile}"));
        }
        if let Some(way) = &r.way {
            line.push_str(&format!("  way: {way}"));
        }
        // What it is held to, when anything: `mcp: github`, `skills: none`.
        let list = |l: &Vec<String>| if l.is_empty() { "none".to_string() } else { l.join(",") };
        for (field, value) in [("mcp", &r.mcp), ("plugins", &r.plugins), ("skills", &r.skills)] {
            if let Some(v) = value {
                line.push_str(&format!("  {field}: {}", list(v)));
            }
        }
        if let Some(t) = &r.tools {
            line.push_str(&format!("  tools: +{} -{}", t.allow.len(), t.deny.len()));
        }
        if let Some(d) = &r.description {
            line.push_str(&format!("  — {d}"));
        }
        if r.problem.is_some() {
            line.push_str("  (cannot start: see below)");
        }
        println!("{}", line.trim_end());
    }
    if !catalog.ways.is_empty() {
        println!();
        println!("Ways of working:");
        for w in &catalog.ways {
            let line = format!(
                "  {:<14}  {:<12}  {}",
                w.name,
                source(w.source),
                w.description.as_deref().unwrap_or("")
            );
            println!("{}", line.trim_end());
        }
    }
    for p in &catalog.problems {
        eprintln!("warning: {p}");
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

/// `error  app.js:12:5  TypeError: x is undefined`, one per entry.
fn render_console(entries: &[serde_json::Value], dropped: u64) {
    if dropped > 0 {
        println!("({dropped} earlier entries dropped)");
    }
    for entry in entries {
        let source = entry["source"].as_str().map(|s| format!("{s}  ")).unwrap_or_default();
        println!(
            "{:<5}  {source}{}",
            entry["level"].as_str().unwrap_or(""),
            entry["message"].as_str().unwrap_or("")
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
        // A select's choices, under it: what `fill` takes.
        for option in node["options"].as_array().into_iter().flatten() {
            println!(
                "      option \"{}\" value=\"{}\"{}",
                option["text"].as_str().unwrap_or(""),
                option["value"].as_str().unwrap_or(""),
                if option["selected"].as_bool() == Some(true) { " selected" } else { "" }
            );
        }
        if let Some(more) = node["moreOptions"].as_u64() {
            println!("      … {more} more options");
        }
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
            transcript_path: payload["transcript_path"].as_str().map(String::from),
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

    fn parse(args: &[&str]) -> Command {
        Cli::try_parse_from(["mira"].iter().chain(args)).unwrap_or_else(|e| panic!("{args:?}: {e}")).command
    }

    #[test]
    fn options_go_before_or_after_a_title_or_a_message() {
        for args in [
            &["task", "add", "Step", "1:", "the", "model", "--to", "worker"][..],
            &["task", "add", "--to", "worker", "Step", "1:", "the", "model"],
            &["task", "add", "Step 1: the model", "--to", "worker"],
        ] {
            let Command::Task { action: TaskAction::Add { title, to, .. } } = parse(args) else { panic!("{args:?}") };
            assert_eq!((title.join(" ").as_str(), to.as_deref()), ("Step 1: the model", Some("worker")), "{args:?}");
        }
        for args in [
            &["agent", "send", "worker", "Step 2", "go", "--card", "ab12", "--wait"][..],
            &["agent", "send", "worker", "--card", "ab12", "--wait", "Step 2", "go"],
        ] {
            let Command::Agent { action: AgentAction::Send { to, message, card, wait, .. } } = parse(args) else {
                panic!("{args:?}")
            };
            assert_eq!(
                (to.as_str(), message.join(" ").as_str(), card.as_deref(), wait),
                ("worker", "Step 2 go", Some("ab12"), true),
                "{args:?}"
            );
        }
        // A card's description, and a title for a message's card.
        let Command::Task { action: TaskAction::Add { title, description, .. } } =
            parse(&["task", "add", "Write the docs", "--description", "README and AGENTS.md"])
        else {
            panic!()
        };
        assert_eq!((title.join(" ").as_str(), description.as_deref()), ("Write the docs", Some("README and AGENTS.md")));
        let Command::Agent { action: AgentAction::Send { title, .. } } =
            parse(&["agent", "send", "worker", "--title", "Focus layout", "Step 3", "all of it"])
        else {
            panic!()
        };
        assert_eq!(title.as_deref(), Some("Focus layout"));
        let Command::Agent { action: AgentAction::New { title, task, .. } } =
            parse(&["agent", "new", "--title", "Docs", "update", "the", "README"])
        else {
            panic!()
        };
        assert_eq!((title.as_deref(), task.join(" ").as_str()), (Some("Docs"), "update the README"));
        // A word that looks like an option is still text after `--`.
        let Command::Task { action: TaskAction::Add { title, to, .. } } = parse(&["task", "add", "--", "drop", "--to"]) else {
            panic!()
        };
        assert_eq!((title.join(" ").as_str(), to), ("drop --to", None));
        let Command::Agent { action: AgentAction::Send { message, card, .. } } =
            parse(&["agent", "send", "worker", "--card", "ab12", "--", "use", "-v"])
        else {
            panic!()
        };
        assert_eq!((message.join(" ").as_str(), card.as_deref()), ("use -v", Some("ab12")));
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

    /// Screenshots taken in the same millisecond each get a file of their
    /// own, and none is overwritten.
    #[test]
    fn new_file_never_reuses_a_path() {
        let dir = std::env::temp_dir().join(format!("mira-new-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let paths: Vec<_> = (0..3).map(|_| new_file(&dir, "shot", "png").unwrap().0).collect();
        let names: Vec<_> = paths.iter().map(|p| p.file_name().unwrap().to_str().unwrap()).collect();
        assert_eq!(names, ["shot.png", "shot-2.png", "shot-3.png"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
