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
    /// Git branch of the focused pane's repository.
    #[serde(default)]
    pub branch: Option<String>,
    /// Pull request linked to that branch (needs `gh`).
    #[serde(default)]
    pub pr: Option<PrStatus>,
    /// TCP ports the tab's processes are listening on.
    #[serde(default)]
    pub ports: Vec<u16>,
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
    FocusPane {
        pane_id: String,
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
    },
    /// Records the agent session running in a pane (for resume-on-restore).
    AgentSession {
        #[serde(default)]
        pane_id: Option<String>,
        #[serde(default)]
        tty: Option<String>,
        agent: String,
        session_id: String,
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
