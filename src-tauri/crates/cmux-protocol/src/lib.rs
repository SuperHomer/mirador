//! Shared types between the app, the CLI, and the TS bindings (mirrored by
//! hand in `src/bindings.ts` until tauri-specta generation lands).
//! Grows with the automation protocol in milestone M6.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDir {
    /// Children sit side by side (a vertical divider between them).
    Row,
    /// Children stack top to bottom (a horizontal divider between them).
    Column,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// The split tree of one tab. Ratios are normalized to sum to 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum Node {
    Leaf {
        pane_id: String,
    },
    Split {
        dir: SplitDir,
        ratios: Vec<f32>,
        children: Vec<Node>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabSnapshot {
    pub id: String,
    /// Resolved title: explicit rename, else OSC title, else basename of
    /// the focused pane's cwd, else "shell".
    pub title: String,
    /// Focused pane's working directory, if known.
    pub cwd: Option<String>,
    pub root: Node,
    pub focused_pane: String,
    /// Unread notifications across the tab's panes.
    #[serde(default)]
    pub unread: u32,
    /// Body of the tab's most recent notification.
    #[serde(default)]
    pub last_notification: Option<String>,
    /// The team running in it, if it is a team's tab.
    #[serde(default)]
    pub team: Option<TeamRun>,
    /// Git branch of the focused pane's repository.
    #[serde(default)]
    pub branch: Option<String>,
    /// Pull request linked to that branch (needs `gh`).
    #[serde(default)]
    pub pr: Option<PrStatus>,
    /// TCP ports the tab's processes are listening on.
    #[serde(default)]
    pub ports: Vec<u16>,
    /// The pane shown alone, filling the tab, when one is.
    #[serde(default)]
    pub zoomed_pane: Option<String>,
    /// The tab shows its agents as the wall instead of its splits.
    #[serde(default)]
    pub wall: bool,
    /// The side of the wall it shows, or will when the wall is next on.
    #[serde(default)]
    pub wall_view: WallView,
    /// How the Agents side lays its agents out.
    #[serde(default)]
    pub wall_mode: WallMode,
    /// How the Tasks side lays its board out.
    #[serde(default)]
    pub task_layout: TaskLayout,
    /// Its agents' panes by urgency — needs you, working, idle, then the
    /// rest, ties in team order: who Focus shows when none is pinned.
    #[serde(default)]
    pub wall_urgency: Vec<String>,
    /// Its agents' panes in team order, cut into the Grid's pages of
    /// `agentsPerTab`; none without agents.
    #[serde(default)]
    pub wall_pages: Vec<Vec<String>>,
    /// Where the tab's frame sits on the board, and its size (derived from
    /// its cards). Card positions are relative to the frame's `x`, `y`.
    #[serde(default)]
    pub frame: BoardRect,
    /// Every pane's card on the board, in the tab's pane order.
    #[serde(default)]
    pub cards: Vec<CardRect>,
}

/// How the workspace is shown: one tab at a time with its splits, or every
/// pane of every tab as a card on one board.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum View {
    #[default]
    Tabs,
    Board,
}

/// The side of a tab's wall on show: its agents' terminals, or the task
/// board of what they were given.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WallView {
    #[default]
    Agents,
    Tasks,
}

impl WallView {
    pub fn is_agents(&self) -> bool {
        *self == WallView::Agents
    }
}

/// How the Agents side of a wall lays its agents out: one large with the
/// rest in a strip beside it, or all of them in an even grid.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WallMode {
    #[default]
    Focus,
    Grid,
}

impl WallMode {
    pub fn is_focus(&self) -> bool {
        *self == WallMode::Focus
    }
}

/// How the Tasks side of a wall lays its board out: a column per status,
/// or a row per agent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskLayout {
    #[default]
    Columns,
    Swimlanes,
}

impl TaskLayout {
    pub fn is_columns(&self) -> bool {
        *self == TaskLayout::Columns
    }
}

/// The part of the board on screen: `x`, `y` is the board point at the
/// viewport's top-left corner, `zoom` the screen pixels per board unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0, zoom: 1.0 }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct BoardPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct BoardRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A pane's card, relative to its tab's frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardRect {
    pub pane_id: String,
    /// What the card's header says the pane is.
    #[serde(default)]
    pub title: String,
    #[serde(flatten)]
    pub rect: BoardRect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrStatus {
    pub number: u64,
    /// OPEN | MERGED | CLOSED
    pub state: String,
    pub url: String,
    /// "pass" | "fail" | "pending" | "none"
    pub checks: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub tabs: Vec<TabSnapshot>,
    pub active_tab: String,
    /// Panes with unread notifications (frontend draws the attention ring).
    #[serde(default)]
    pub unread_panes: Vec<String>,
    /// Command panes (frontend draws the 🤖 chip).
    #[serde(default)]
    pub agent_panes: Vec<AgentPane>,
    /// Browser panes and their current URLs.
    #[serde(default)]
    pub browser_panes: Vec<BrowserPane>,
    /// Remote (SSH) panes and their display host.
    #[serde(default)]
    pub remote_panes: Vec<RemotePane>,
    /// Diff panes and what each one is showing.
    #[serde(default)]
    pub diff_panes: Vec<DiffPane>,
    /// Release-notes panes and the version each one shows.
    #[serde(default)]
    pub whats_new_panes: Vec<WhatsNewPane>,
    /// Commit-graph panes and the repository each one reads.
    #[serde(default)]
    pub graph_panes: Vec<GraphPane>,
    /// Agent-editor panes: each edits the role, way and team files.
    #[serde(default)]
    pub agent_editor_panes: Vec<AgentEditorPane>,
    /// Every pane running a Claude Code agent, for the wall to draw.
    #[serde(default)]
    pub agents: Vec<AgentInfo>,
    #[serde(default)]
    pub view: View,
    #[serde(default)]
    pub camera: Camera,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPane {
    pub pane_id: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffPane {
    pub pane_id: String,
    /// Repository root the diff is taken in.
    pub repo: String,
    /// What is being diffed: "worktree", "staged", or a revspec.
    pub spec: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemotePane {
    pub pane_id: String,
    /// Display label (destination), e.g. "user@box".
    pub host: String,
}

/// Automation socket request. One JSON object per line; `pane_id: None`
/// targets the focused pane of the active tab.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    ListTabs,
    NewTab {
        #[serde(default)]
        command: Option<String>,
    },
    SplitPane {
        #[serde(default)]
        pane_id: Option<String>,
        dir: SplitDir,
        #[serde(default)]
        command: Option<String>,
    },
    ClosePane {
        pane_id: String,
    },
    /// Closes a tab and every pane in it.
    CloseTab {
        tab_id: String,
    },
    FocusPane {
        pane_id: String,
    },
    /// Shows a pane alone, filling its tab; `zoom: false` shows the whole
    /// tab again, and without `zoom` it toggles.
    ZoomPane {
        #[serde(default)]
        pane_id: Option<String>,
        #[serde(default)]
        zoom: Option<bool>,
    },
    /// Shows the workspace as `view`; without it, only answers which view
    /// is shown.
    View {
        #[serde(default)]
        view: Option<View>,
    },
    /// Where the board is looked at. Saved, never broadcast: the board
    /// that moved it already shows it.
    SetCamera {
        x: f64,
        y: f64,
        zoom: f64,
        /// The board's size on screen: with the camera, where a new frame
        /// goes (where it is being looked at).
        #[serde(default)]
        width: Option<f64>,
        #[serde(default)]
        height: Option<f64>,
    },
    /// Moves or resizes a pane's card, relative to its tab's frame. Refused
    /// (left where it was) over another card, or growing its frame into
    /// another frame; answers where the card is.
    SetCardRect {
        pane_id: String,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
    /// Moves a tab's frame, its cards with it; refused onto another frame.
    /// Answers where the frame is.
    SetFramePosition {
        tab_id: String,
        x: f64,
        y: f64,
    },
    SendInput {
        #[serde(default)]
        pane_id: Option<String>,
        data: String,
    },
    ReadScreen {
        #[serde(default)]
        pane_id: Option<String>,
        /// Trailing buffer lines to return (default: the visible screen).
        #[serde(default)]
        lines: Option<u32>,
    },
    Notify {
        #[serde(default)]
        pane_id: Option<String>,
        /// Target the pane whose shell owns this tty (e.g. "ttys004") —
        /// how hooks running inside a pane address their own pane.
        #[serde(default)]
        tty: Option<String>,
        #[serde(default)]
        title: Option<String>,
        body: String,
        /// Claude Code daemon job id, for a hook from a background session:
        /// its tty and inherited pane id are the daemon's, not a pane's.
        #[serde(default)]
        job: Option<String>,
    },
    /// Quits the app. With `end_sessions`, every terminal session is ended
    /// first, held ones included; without it, held sessions keep running
    /// for the next launch (as with Cmd+Q).
    Quit {
        #[serde(default)]
        end_sessions: bool,
    },
    /// Records the agent session running in a pane (for resume-on-restore),
    /// and — from a CLI that sends them — what the hook says it is doing.
    AgentSession {
        #[serde(default)]
        pane_id: Option<String>,
        #[serde(default)]
        tty: Option<String>,
        agent: String,
        session_id: String,
        /// See `Notify::job`.
        #[serde(default)]
        job: Option<String>,
        /// The `mira claude-hook` event ("stop", "prompt-submit", …).
        /// Absent from older CLIs, which then only record the session.
        #[serde(default)]
        event: Option<String>,
        /// The session's transcript (`transcript_path` in the hook's
        /// payload): where its subagents' transcripts are found.
        #[serde(default)]
        transcript_path: Option<String>,
        /// A Notification hook's `notification_type` ("permission_prompt",
        /// "idle_prompt", …), when Claude Code sends one.
        #[serde(default)]
        notification_type: Option<String>,
        /// A Notification hook's message.
        #[serde(default)]
        message: Option<String>,
    },
    /// Starts an agent in a new pane (a split of the calling pane, or a new
    /// tab): plain Claude Code, or a role's harness, model and prompt.
    AgentNew {
        /// A role the calling pane's directory sees (`RoleCatalog`); none
        /// starts plain `claude`, or `harness`.
        #[serde(default)]
        role: Option<String>,
        /// Without a role, what to start: Claude Code (default) or Codex.
        #[serde(default)]
        harness: Option<Harness>,
        /// The title of the task's card, instead of one made from `task`.
        #[serde(default)]
        title: Option<String>,
        /// The first prompt, typed in as Claude's initial message.
        #[serde(default)]
        task: Option<String>,
        /// "split" (default) or "tab".
        #[serde(default)]
        target: Option<String>,
        /// The pane a split comes from, and whose directory the agent
        /// starts in; the focused pane when absent.
        #[serde(default)]
        pane_id: Option<String>,
    },
    /// Every agent pane and what it is doing.
    AgentList,
    /// A Claude Code agent's progress and its subagents, from its
    /// transcripts: `pane_id`'s agent, or the focused pane's.
    AgentActivity {
        #[serde(default)]
        pane_id: Option<String>,
    },
    /// Shows the tab of `pane_id` (the focused pane when absent) as the
    /// wall of its agents, or its splits again when it already is. With a
    /// `mode`, shows the wall's agents in that layout instead, turning it
    /// on if it is off. Answers `{"wall": bool, "wallMode": …}`.
    AgentWall {
        #[serde(default)]
        pane_id: Option<String>,
        #[serde(default)]
        mode: Option<WallMode>,
    },
    /// Shows the tab of `pane_id` (the focused pane's when absent) as its
    /// wall, on the Tasks side: its task board. Answers `{"wall": true,
    /// "wallView": "tasks"}`.
    AgentBoard {
        #[serde(default)]
        pane_id: Option<String>,
    },
    /// Opens the editor for the role, way and team files `pane_id`'s
    /// directory sees: a split of that pane, or a tab with `target` "tab".
    AgentEditor {
        #[serde(default)]
        pane_id: Option<String>,
        #[serde(default)]
        target: Option<String>,
    },
    /// Types `message` into another agent's terminal as its next prompt:
    /// `to` is a teammate's name in the caller's tab (`pane_id`'s) — a team
    /// member's, a session's, a role's — or any pane's id. Refused for a
    /// pane running no agent, and for one waiting on a person unless
    /// `force`.
    AgentSend {
        to: String,
        message: String,
        #[serde(default)]
        pane_id: Option<String>,
        #[serde(default)]
        force: bool,
        /// A card written by hand the message is for, by id or a prefix of
        /// one: it is logged on that card rather than on one of its own.
        #[serde(default)]
        card: Option<String>,
        /// The title of the message's card, instead of one made from it.
        #[serde(default)]
        title: Option<String>,
    },
    /// The pane `name` means from `pane_id`'s tab, as `AgentSend` finds it.
    AgentFind {
        name: String,
        #[serde(default)]
        pane_id: Option<String>,
    },
    /// Starts a team in a tab of its own: every member, laid out, each
    /// told who its teammates are. `task` goes to the lead. The team is
    /// the one `pane_id`'s directory sees.
    TeamStart {
        team: String,
        #[serde(default)]
        task: Option<String>,
        #[serde(default)]
        pane_id: Option<String>,
        /// The title of the task's card, instead of one made from `task`.
        #[serde(default)]
        title: Option<String>,
    },
    /// Every running team: its tab, and each member's pane and state.
    TeamList,
    /// The task board of `pane_id`'s tab, or of every tab with `all`.
    TaskList {
        #[serde(default)]
        all: bool,
        #[serde(default)]
        pane_id: Option<String>,
    },
    /// Writes a card in `pane_id`'s tab, for the agent `to` (as
    /// `AgentSend` finds it) or nobody yet. Answers its id.
    TaskAdd {
        title: String,
        #[serde(default)]
        to: Option<String>,
        #[serde(default)]
        pane_id: Option<String>,
        /// More about it than the title says.
        #[serde(default)]
        description: Option<String>,
    },
    /// Puts a card written by hand in `column`; `card` is its id, or a
    /// prefix of one card's only.
    TaskMove {
        card: String,
        column: TaskColumn,
        #[serde(default)]
        pane_id: Option<String>,
    },
    /// The task log, oldest first: of the agent `name` (as `AgentSend`
    /// finds it), else of `pane_id`'s tab, or all of it with `all`.
    AgentHistory {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        all: bool,
        #[serde(default)]
        pane_id: Option<String>,
        /// The history of one card instead, by id or a prefix of one.
        #[serde(default)]
        card: Option<String>,
    },
    /// What an agent did, turn by turn: of the agent `name` (as
    /// `AgentSend` finds it), else of `pane_id`. Only its last `turns`
    /// turns when given.
    AgentTrail {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        pane_id: Option<String>,
        #[serde(default)]
        turns: Option<u32>,
    },
    /// One action of that trail in full, by its id.
    AgentTrailDetail {
        action_id: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        pane_id: Option<String>,
    },
    /// Adds a member to the team of `pane_id`'s tab, beside the others.
    TeamAdd {
        role: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        pane_id: Option<String>,
    },
    /// Starts again the members of `pane_id`'s team whose agent has exited
    /// or whose pane was closed — `members` by name, or all of them.
    TeamRestart {
        #[serde(default)]
        members: Vec<String>,
        #[serde(default)]
        pane_id: Option<String>,
    },
    /// Agent-visible command execution: opens a command pane (split of the
    /// focused pane, or a new tab) whose PTY runs the command directly —
    /// exit detection, output capture, and human interruption all work.
    Run {
        command: String,
        /// "split" (default) or "tab"
        #[serde(default)]
        target: Option<String>,
        /// Block until the command exits; returns exit code + clean output.
        #[serde(default)]
        wait: bool,
        /// Wait timeout in seconds (default 600).
        #[serde(default)]
        timeout_secs: Option<u64>,
    },
    /// Command-pane run history (the agent activity audit log).
    ListRuns,
    /// Opens a browser pane (split of the focused pane, or a new tab).
    BrowserOpen {
        url: String,
        #[serde(default)]
        target: Option<String>,
    },
    /// The Browser* verbs below default to the active tab's browser pane.
    BrowserNavigate {
        #[serde(default)]
        pane_id: Option<String>,
        url: String,
    },
    BrowserSnapshot {
        #[serde(default)]
        pane_id: Option<String>,
    },
    /// The page as a PNG, base64 in `png`: the part the pane shows, at
    /// the display's resolution.
    BrowserScreenshot {
        #[serde(default)]
        pane_id: Option<String>,
    },
    BrowserClick {
        #[serde(default)]
        pane_id: Option<String>,
        /// Numeric id from a snapshot, or a CSS selector.
        target: String,
    },
    BrowserFill {
        #[serde(default)]
        pane_id: Option<String>,
        target: String,
        value: String,
    },
    BrowserEval {
        #[serde(default)]
        pane_id: Option<String>,
        js: String,
    },
    BrowserHistory {
        #[serde(default)]
        pane_id: Option<String>,
        /// "back" | "forward" | "reload"
        action: String,
    },
    /// The visible text of the element `selector` matches, or of the page.
    BrowserText {
        #[serde(default)]
        pane_id: Option<String>,
        #[serde(default)]
        selector: Option<String>,
    },
    /// Types `text` into `target` key by key, at the cursor; a field or
    /// editable element not focused gets the cursor at its end first.
    BrowserType {
        #[serde(default)]
        pane_id: Option<String>,
        target: String,
        text: String,
    },
    /// Presses one key or chord ("Enter", "Shift+Tab", "Meta+A") on the
    /// focused element, or on `target`.
    BrowserPress {
        #[serde(default)]
        pane_id: Option<String>,
        key: String,
        #[serde(default)]
        target: Option<String>,
    },
    /// What the page logged to its console, and its errors, since it
    /// loaded: `errors` keeps only errors; `clear` empties the log after.
    BrowserConsole {
        #[serde(default)]
        pane_id: Option<String>,
        #[serde(default)]
        errors: bool,
        #[serde(default)]
        clear: bool,
    },
    /// Waits until the page meets one condition: `selector` shown, `text`
    /// in its visible text, the loaded URL containing `url`, or `gone` no
    /// longer shown. Answers nothing when met; an error on timeout.
    BrowserWait {
        #[serde(default)]
        pane_id: Option<String>,
        #[serde(default)]
        selector: Option<String>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        gone: Option<String>,
        /// Seconds (default 10).
        #[serde(default)]
        timeout_secs: Option<u64>,
    },
    /// Opens a remote (SSH) pane running `ssh -tt <host>`.
    SshOpen {
        /// ssh host spec (alias, or full destination with options).
        host: String,
        #[serde(default)]
        target: Option<String>,
    },
    /// Host aliases from ~/.ssh/config.
    SshHosts,
    /// Forward (or cancel) `localhost:<port>` → remote's `localhost:<port>`
    /// over a remote pane's ControlMaster.
    SshForward {
        #[serde(default)]
        pane_id: Option<String>,
        port: u16,
        #[serde(default)]
        cancel: bool,
    },
    /// Opens a commit-graph pane (split of the source pane, or a new tab),
    /// for the repository the calling pane sits in.
    GraphOpen {
        #[serde(default)]
        target: Option<String>,
        #[serde(default)]
        pane_id: Option<String>,
    },
    /// Lists the checkouts of the repository the calling pane sits in, so
    /// an agent can name one for `DiffOpen`.
    DiffWorktrees {
        #[serde(default)]
        pane_id: Option<String>,
    },
    /// Opens a diff pane (split of the source pane, or a new tab). The
    /// repository comes from the source pane's cwd: the calling pane when
    /// the CLI knows it, else whichever pane is focused.
    DiffOpen {
        /// "worktree" (default), "staged", or a revspec ("abc123",
        /// "main...HEAD").
        #[serde(default)]
        spec: Option<String>,
        #[serde(default)]
        target: Option<String>,
        /// The pane `mira diff` ran in. Absent from older clients, and
        /// when the environment is lost (sudo, a detached session).
        #[serde(default)]
        pane_id: Option<String>,
        /// Another checkout of the same repository to review instead of the
        /// calling pane's, by path or branch name.
        #[serde(default)]
        worktree: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub id: String,
    pub pane_id: String,
    pub command: String,
    /// Unix millis.
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPane {
    pub pane_id: String,
    pub command: String,
}

/// A coding-agent program Mirador recognizes in a pane's process table.
/// Claude Code is the one with hooks (status, resume on restart); the rest
/// are counted and shown as agents from their process alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Harness {
    #[default]
    Claude,
    Codex,
    OpenCode,
    Pi,
    Gemini,
    Amp,
    CursorAgent,
    Copilot,
    Aider,
    Goose,
    Qwen,
    Crush,
    Droid,
}

impl Harness {
    /// The name people know it by.
    pub fn label(self) -> &'static str {
        match self {
            Harness::Claude => "Claude Code",
            Harness::Codex => "Codex",
            Harness::OpenCode => "OpenCode",
            Harness::Pi => "Pi",
            Harness::Gemini => "Gemini CLI",
            Harness::Amp => "Amp",
            Harness::CursorAgent => "Cursor Agent",
            Harness::Copilot => "Copilot CLI",
            Harness::Aider => "Aider",
            Harness::Goose => "Goose",
            Harness::Qwen => "Qwen Code",
            Harness::Crush => "Crush",
            Harness::Droid => "Droid",
        }
    }
}

/// What a Claude Code agent is doing, as its hooks last reported it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentStatus {
    /// A prompt was submitted, or a tool ran: Claude is on it.
    Working,
    /// Blocked on the human: a permission prompt, a question.
    NeedsYou,
    /// Finished its turn, waiting for the next prompt.
    Idle,
}

/// A pane running a coding agent: Claude Code, or another [`Harness`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub pane_id: String,
    /// The tab holding it.
    #[serde(default)]
    pub tab_id: String,
    /// The program it runs; Claude Code for anything its hooks or a role
    /// made an agent before the process was seen.
    #[serde(default)]
    pub harness: Harness,
    /// The `agentRoles` entry it was started with.
    pub role: Option<String>,
    /// Its name in its tab's team (`reviewer`, `dev-2`), when it is a
    /// member: what it is called first, whatever its harness — a Codex
    /// member has no session name to go by.
    #[serde(default)]
    pub member: Option<String>,
    /// Whether it has activity to show (`agent activity`, the panel, the
    /// wall's list): a Claude Code agent whose hooks said where its
    /// transcript is, or a Codex agent whose rollout Mirador has found.
    /// The one place that rule lives (`PaneMeta::has_activity`).
    #[serde(default)]
    pub has_activity: bool,
    /// The session's name, as `claude agents` lists it: one given with
    /// `--name` or `/rename`, or one Claude Code made up.
    #[serde(default)]
    pub name: Option<String>,
    /// The role's model, or the `--model` (`-m`) its process was started
    /// with; absent when it runs the harness's default.
    pub model: Option<String>,
    /// Its role's effort ("high", "max"); absent without a role, or when
    /// the role sets none.
    #[serde(default)]
    pub effort: Option<String>,
    /// What its hooks last said, or else what Claude Code's session file
    /// says (busy or idle); always absent for a harness other than Claude
    /// Code.
    pub status: Option<AgentStatus>,
    /// The last Notification message, e.g. what it needs permission for.
    pub message: Option<String>,
    /// The tool call a Claude Code agent waits on a permission for, read
    /// from its transcript when it started waiting; absent otherwise.
    #[serde(default)]
    pub asking: Option<Asking>,
    /// What its own terminal title says it is on — the session's name or a
    /// task summary — when that adds to the role (`agents::title_topic`).
    #[serde(default)]
    pub topic: Option<String>,
    /// Unix millis of the last status change.
    pub since_ms: Option<u64>,
    pub cwd: Option<String>,
    pub branch: Option<String>,
}

/// How far an agent is through the task list it keeps (`TaskCreate` /
/// `TaskUpdate`, or an older `TodoWrite`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskProgress {
    pub done: u32,
    pub total: u32,
    /// What the task in progress says it is doing (its `activeForm`, or
    /// its subject).
    pub current: Option<String>,
}

/// Where a subagent is, as its transcript and its session's process say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SubagentState {
    Running,
    /// Its last turn ended, or it handed back its report. It can be sent
    /// a message and run again.
    Done,
    /// Was running when the Claude Code hosting it exited.
    Stopped,
}

/// A tool call an agent asks permission for: its tool, and what it is
/// about in a line, as its trail sums it up (a command, a path).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asking {
    pub tool: String,
    pub summary: String,
}

/// What one transcript adds up to: the model, the tokens, the tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSummary {
    /// The model of its last reply.
    pub model: Option<String>,
    /// Tokens it processed afresh: input not read from the cache, plus
    /// output, over every reply.
    pub tokens: u64,
    /// The size of its context at the last reply.
    pub context: u64,
    /// How large that context can grow, for its model, when known
    /// (`cmux_core::transcript::context_window`).
    #[serde(default)]
    pub context_window: Option<u64>,
    pub tool_calls: u32,
    /// Absent when it keeps no task list.
    pub tasks: Option<TaskProgress>,
    /// Unix millis of its first and latest entries.
    pub started_ms: Option<u64>,
    pub last_ms: Option<u64>,
}

/// A subagent of a Claude Code session (`Agent` tool), from its transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentInfo {
    pub id: String,
    /// What it was asked to do, as the `Agent` call described it.
    pub description: Option<String>,
    /// Its `subagent_type`, e.g. `Explore`.
    pub agent_type: Option<String>,
    /// 1 for a subagent of the session, 2 for one of those's, …
    pub depth: u32,
    pub state: SubagentState,
    #[serde(flatten)]
    pub summary: TranscriptSummary,
}

/// What an agent pane's Claude Code session and its subagents are doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentActivity {
    pub pane_id: String,
    /// The session itself.
    pub session: TranscriptSummary,
    /// Running ones first, then the rest, latest activity first.
    pub subagents: Vec<SubagentInfo>,
}

/// What an agent pane's agent did, from its own transcript (Claude Code)
/// or rollout (Codex), oldest turn first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTrail {
    pub pane_id: String,
    pub turns: Vec<TrailTurn>,
    /// Turns before the first one in `turns`.
    pub older: u32,
    /// Turns left out between the first one — a card's, older than the
    /// latest — and the latest; 0 when they follow on.
    pub gap: u32,
    /// What it is a reading of: an answer with the version of the last
    /// is the same answer.
    pub version: Option<String>,
    /// Why there is nothing to show, when there is not: no transcript
    /// found, an agent without one, more than one Codex session.
    pub reason: Option<String>,
}

/// One turn of an agent's trail: a prompt and what the agent did for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrailTurn {
    /// Its first line, shortened. Absent for actions with no prompt before
    /// them (a transcript that starts mid-conversation).
    pub prompt: Option<String>,
    /// The whole prompt, up to a few thousand characters.
    pub text: Option<String>,
    /// Unix millis of its first and latest entries.
    pub started_ms: Option<u64>,
    pub last_ms: Option<u64>,
    /// In the order they were called.
    pub actions: Vec<TrailAction>,
}

/// One tool call, summed up: the full input and output stay in the agent's
/// transcript, read on demand as an [`ActionDetail`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrailAction {
    /// The tool call's id, which asks for its detail.
    pub id: String,
    pub tool: String,
    /// One line: a command, a path, a pattern, a URL, a description.
    pub summary: String,
    pub started_ms: Option<u64>,
    /// From the call to its result; absent until there is one.
    pub duration_ms: Option<u64>,
    pub status: ActionStatus,
    /// A failed command's, when its result says it.
    pub exit_code: Option<i32>,
    /// The first lines of its result, shortened.
    pub output: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActionStatus {
    Ok,
    Error,
    /// No result yet.
    Running,
    /// No result, and the agent moved on: interrupted, or the session
    /// was stopped under it.
    Interrupted,
}

/// One tool call in full, as its transcript has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionDetail {
    pub id: String,
    pub tool: String,
    /// Its input, as pretty-printed JSON.
    pub input: String,
    /// Its result's text; absent while there is none.
    pub output: Option<String>,
    pub is_error: bool,
}

/// A named way to start an agent: a Markdown file in a repository's
/// `.mirador/roles` or the config directory's `roles`, or an `agentRoles`
/// entry in mirador.json.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentRole {
    pub name: String,
    /// What it starts: Claude Code unless the role says otherwise.
    pub harness: Harness,
    /// `--model`: an alias ("opus", "sonnet") or a full model name.
    pub model: Option<String>,
    /// How hard its model thinks: Claude Code's `--effort`, Codex's
    /// `model_reasoning_effort` ("high", "max").
    pub effort: Option<String>,
    /// Codex's `--profile`: the person's own `<name>.config.toml` beside
    /// Codex's config.toml, for what a role cannot say itself (approvals,
    /// sandbox).
    pub profile: Option<String>,
    /// The role's own instructions, added to the harness's system prompt.
    pub prompt: Option<String>,
    /// The way of working it follows, by name: its prompt comes after the
    /// role's.
    pub way: Option<String>,
    /// One line saying what it is for, shown beside its name.
    pub description: Option<String>,
    /// Only these MCP servers, by name, wherever they are configured; none
    /// when empty. Absent, every server the agent would have anyway.
    pub mcp: Option<Vec<String>>,
    /// Only these plugins (`name` or `name@marketplace`); none when empty.
    pub plugins: Option<Vec<String>>,
    /// Only these skills (`pdf`, or `anthropic-skills:pdf`); none when
    /// empty, the built-in ones included.
    pub skills: Option<Vec<String>>,
    /// Permission rules, in Claude Code's own syntax (`Bash(git push *)`).
    pub tools: Option<ToolRules>,
    /// Where it was defined. Set by whoever loads it, never read from it.
    #[serde(skip_deserializing)]
    pub source: RoleSource,
    /// The file it came from; none for an `agentRoles` entry.
    #[serde(skip_deserializing)]
    pub path: Option<String>,
    /// Why it cannot start, when it cannot: a way of working nobody wrote,
    /// a harness a role cannot start yet. Listed anyway — it is still the
    /// role you wrote — so this says what to fix.
    #[serde(skip_deserializing)]
    pub problem: Option<String>,
}

/// A role's permission rules: what it may do without asking, and what it
/// may never do.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolRules {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

/// A way of working: a playbook any role can follow, from `ways/<name>.md`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWay {
    pub name: String,
    pub prompt: String,
    pub description: Option<String>,
    pub source: RoleSource,
    pub path: String,
}

/// A team: roles started together in one tab, from `teams/<name>.md`. Its
/// body is the team's charter, given to every member after its role's own
/// prompt, with the names of its teammates.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTeam {
    pub name: String,
    pub description: Option<String>,
    /// A way of working every member follows, besides its role's own.
    pub way: Option<String>,
    pub members: Vec<TeamMember>,
    /// How the members share the tab: "grid" (default), "columns", "rows".
    pub layout: Option<String>,
    pub prompt: Option<String>,
    pub source: RoleSource,
    pub path: String,
    /// Why it cannot start, when it cannot.
    pub problem: Option<String>,
}

/// One line of a team's `members`: a role, how many of it, and what to
/// call them (`dev` twice is `dev-1` and `dev-2`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMember {
    pub role: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default = "one")]
    pub count: u32,
    /// The member a team's task goes to.
    #[serde(default)]
    pub lead: bool,
}

fn one() -> u32 {
    1
}

/// A team running in a tab: which pane each member is in. Kept with the
/// tab, across restarts, so the team can be listed, restarted, and grown.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamRun {
    pub name: String,
    pub members: Vec<TeamSeat>,
}

/// A member of a running team: its name, its role, and its pane — which
/// may since have been closed.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamSeat {
    pub name: String,
    pub role: String,
    pub pane: String,
    #[serde(default)]
    pub lead: bool,
}

/// A running team, as `mira team list` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamInfo {
    pub tab_id: String,
    pub name: String,
    pub members: Vec<TeamMemberInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamMemberInfo {
    pub name: String,
    pub role: String,
    pub pane: String,
    pub lead: bool,
    pub state: SeatState,
    /// What its hooks or session file say, while it runs.
    pub status: Option<AgentStatus>,
}

/// Where a member is: its agent running, exited to a shell, or its pane
/// closed. `team restart` brings back the last two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SeatState {
    Running,
    Exited,
    Closed,
}

/// A column of the task board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskColumn {
    Todo,
    Doing,
    NeedsYou,
    Done,
}

/// How a task was given: by Mirador starting a team or an agent, by
/// `mira agent send`, or written as a card with `mira task add`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskSource {
    TeamStart,
    AgentNew,
    AgentSend,
    Manual,
}

/// How an agent's turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnEnd {
    /// It stopped and waits for its next prompt.
    Done,
    /// It stopped to ask a person something.
    NeedsYou,
    /// Its agent is gone: the pane is a shell again, or closed.
    Exited,
}

/// One line of the task log: something that happened to a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskEvent {
    /// Unix millis.
    pub at_ms: u64,
    #[serde(flatten)]
    pub what: TaskEventKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TaskEventKind {
    /// A task given to an agent, or a card written.
    Given {
        card: String,
        source: TaskSource,
        /// The sender's name, or `you` for a person.
        from: String,
        /// The agent it was given to, by name and pane; a card written
        /// for nobody yet has neither.
        to: Option<String>,
        pane: Option<String>,
        team: Option<String>,
        tab: Option<String>,
        text: String,
        /// The card's title when one was given (`--title`, a card written
        /// by hand); else it comes from `text`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// More about a card written by hand (`task add --description`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    /// A card put in a column by hand.
    Moved {
        card: String,
        column: TaskColumn,
        /// Who moved it, as in `from`.
        by: String,
    },
    /// The agent in `pane` stopped.
    TurnEnded { pane: String, end: TurnEnd },
    /// The agent in `pane` went back to work after needing a person.
    Resumed { pane: String },
}

/// One line of `mira agent history`: an event of the log, with what the
/// rest of the log says about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskHistoryEntry {
    #[serde(flatten)]
    pub event: TaskEvent,
    /// The agent a turn end or a resume is about, by the name it was last
    /// given a task under.
    pub agent: Option<String>,
    /// For a turn end: how long since the task or the resume it ends.
    pub turn_ms: Option<u64>,
    /// The event in one line, as `mira agent history` prints it after its
    /// age: `planner → worker  agent-send  Step 2: …`, `worker done after
    /// 3m`.
    #[serde(default)]
    pub summary: String,
}

/// A card of the task board: the log folded up, one card per task given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCard {
    pub id: String,
    /// The title it was given, or else the first line of the task's text,
    /// cut short (`cmux_core::tasks::split_title`).
    pub title: String,
    /// The rest: the whole task when it says more than the title.
    #[serde(default)]
    pub description: Option<String>,
    pub source: TaskSource,
    pub from: String,
    pub assignee: Option<String>,
    pub pane: Option<String>,
    pub team: Option<String>,
    pub tab: Option<String>,
    pub column: TaskColumn,
    pub created_ms: u64,
    pub updated_ms: u64,
    /// The events that touched it, oldest first.
    pub history: Vec<TaskEvent>,
    /// For a card in Doing: the failed actions in its agent's latest turn,
    /// from its trail; 0 when that is not known.
    #[serde(default)]
    pub failing: u32,
}

/// Where a role or a way of working was defined. A repository's wins over
/// the user's, a file over an `agentRoles` entry of the same name, and any
/// of them over one that ships with Mirador.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RoleSource {
    /// Shipped with Mirador, the same for everyone (`cmux_core::builtin`).
    Builtin,
    /// `agentRoles` in mirador.json.
    #[default]
    Config,
    /// A file in the config directory's `roles` or `ways`.
    Global,
    /// A file in the repository's `.mirador/roles` or `.mirador/ways`.
    Repo,
}

/// The roles and ways of working a directory sees, and what was wrong with
/// the files that did not make it in.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleCatalog {
    pub roles: Vec<AgentRole>,
    pub ways: Vec<AgentWay>,
    #[serde(default)]
    pub teams: Vec<AgentTeam>,
    /// `<file>: <what is wrong>`, one per problem.
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEnvelope {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(flatten)]
    pub req: Request,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    #[serde(default)]
    pub id: Option<u64>,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationDto {
    pub id: String,
    pub pane_id: String,
    pub title: Option<String>,
    pub body: String,
    /// Unix millis.
    pub at_ms: u64,
    pub read: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomCommand {
    pub name: String,
    pub command: String,
    /// Where the command runs: a new "tab", or a "split" of the focused pane.
    #[serde(default = "default_command_target")]
    pub target: String,
}

fn default_command_target() -> String {
    "split".to_string()
}

/// Terminal colors after theme resolution. All values are `#rrggbb`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedColors {
    pub background: String,
    pub foreground: String,
    pub cursor: String,
    pub selection_background: String,
    /// 16 ANSI colors (normal 0-7, bright 8-15).
    pub palette: Vec<String>,
}

/// Fully-resolved runtime configuration pushed to the frontend on load and
/// on every hot-reload (`config-changed` event).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedConfig {
    pub font_family: String,
    pub font_size: f32,
    pub scrollback: u32,
    pub mac_option_is_meta: bool,
    pub colors: ResolvedColors,
    /// accelerator ("mod+shift+d") → action id ("split_down")
    pub keybindings: std::collections::HashMap<String, String>,
    pub custom_commands: Vec<CustomCommand>,
    pub agent_roles: Vec<AgentRole>,
    /// How many agents a page of the wall's Grid layout holds: 4, 6 or 9.
    pub agents_per_tab: u8,
    pub agent_click: AgentClick,
}

/// What clicking an agent in the sidebar does: show its tab's wall in
/// Focus with that agent pinned, or show its pane alone, filling its tab.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentClick {
    #[default]
    Wall,
    Pane,
}

/// One line of a diff hunk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    /// "context" | "add" | "del" | "meta" ("\ No newline at end of file").
    pub kind: String,
    /// Line number on the old side (None for additions).
    pub old_line: Option<u32>,
    /// Line number on the new side (None for deletions).
    pub new_line: Option<u32>,
    /// Line text, without the leading +/-/space marker.
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffHunk {
    pub old_start: u32,
    pub new_start: u32,
    /// Text trailing the `@@ … @@` marker — git's guess at the enclosing
    /// function or section.
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFile {
    pub path: String,
    /// Where a rename or copy came from.
    pub old_path: Option<String>,
    /// "added" | "deleted" | "renamed" | "copied" | "modified" | "untracked"
    pub status: String,
    pub additions: u32,
    pub deletions: u32,
    pub binary: bool,
    /// Body dropped for being too large; the counts are still exact.
    pub truncated: bool,
    pub hunks: Vec<DiffHunk>,
}

/// What a diff pane renders.
/// A ref pointing at a commit, as the graph labels it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphRef {
    pub name: String,
    /// "head" | "branch" | "remote" | "tag"
    pub kind: String,
}

/// One segment of a branch line, drawn between a row and the row below it:
/// `from` is its lane on this row, `to` its lane on the next. Equal lanes
/// are a straight line down; differing ones a branch or a merge.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphLink {
    pub from: u32,
    pub to: u32,
}

/// One commit, placed in a lane, with the lines leaving its row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphRow {
    pub sha: String,
    pub short: String,
    pub subject: String,
    pub author: String,
    /// Author time, seconds since the epoch.
    pub timestamp: i64,
    pub refs: Vec<GraphRef>,
    /// Which lane the commit's dot sits in.
    pub lane: u32,
    pub links: Vec<GraphLink>,
    /// More than one parent, so the dot is drawn hollow.
    pub merge: bool,
}

/// A repository's commit graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphResult {
    pub repo: String,
    pub rows: Vec<GraphRow>,
    /// Lanes in use, so the view can size its gutter once.
    pub lanes: u32,
    /// History was cut at the row limit; older commits exist.
    pub truncated: bool,
}

/// A pane editing the role, way and team files of the directory it was
/// opened from (its pane's `cwd`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEditorPane {
    pub pane_id: String,
    pub cwd: Option<String>,
}

/// A pane showing a commit graph, and the repository it reads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphPane {
    pub pane_id: String,
    pub repo: String,
}

/// One inline run inside a release note's text. Carries text, never markup:
/// the frontend maps each variant to a React element, so remote note text is
/// never interpreted as HTML in the privileged webview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NoteSpan {
    Text { text: String },
    Code { text: String },
    Strong { text: String },
    Em { text: String },
    /// Only `http(s)` links become links; anything else stays Text.
    Link { text: String, href: String },
}

/// One block of a release note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NoteBlock {
    Heading { level: u8, spans: Vec<NoteSpan> },
    /// A screenshot or GIF on a line of its own. The bytes are fetched and
    /// cached separately, by `url`; `alt` is what stands in for it while
    /// loading, or if it never arrives.
    Image { alt: String, url: String },
    Paragraph { spans: Vec<NoteSpan> },
    List { items: Vec<Vec<NoteSpan>> },
    Code { text: String, lang: Option<String> },
    Rule,
}

/// A pane showing release notes, and which version's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsNewPane {
    pub pane_id: String,
    pub version: String,
    /// Set when the pane covers every release after this version, up to
    /// `version` — an update that skipped some.
    #[serde(default)]
    pub since: Option<String>,
}

/// One checkout of a repository, as `git worktree list` reports it.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    /// Absolute path of the checkout — the identity a diff pane stores.
    pub path: String,
    /// Short branch name ("feature/login"), absent when detached or bare.
    pub branch: Option<String>,
    /// Short HEAD sha, absent on a bare repository.
    pub head: Option<String>,
    /// The repository's original checkout, as opposed to a linked one.
    pub main: bool,
    /// A bare repository has no files to diff.
    pub bare: bool,
    /// `git worktree lock` — shown so a pane can say why writes may fail.
    pub locked: bool,
    /// Git considers this entry stale (its directory is gone).
    pub prunable: bool,
    /// The checkout the asking pane is currently reading.
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffResult {
    pub repo: String,
    /// The spec as requested ("worktree", "staged", or a revspec).
    pub spec: String,
    /// Header line: "uncommitted changes", a commit subject, a range.
    pub label: String,
    pub files: Vec<DiffFile>,
}
