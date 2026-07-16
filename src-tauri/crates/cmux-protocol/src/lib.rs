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
        #[serde(default)]
        title: Option<String>,
        body: String,
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
    pub colors: ResolvedColors,
    /// accelerator ("mod+shift+d") → action id ("split_down")
    pub keybindings: std::collections::HashMap<String, String>,
    pub custom_commands: Vec<CustomCommand>,
}
