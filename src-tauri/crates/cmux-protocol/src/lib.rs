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
    /// Resolved title: explicit rename, else basename of the focused pane's
    /// cwd, else "shell".
    pub title: String,
    /// Focused pane's working directory, if known.
    pub cwd: Option<String>,
    pub root: Node,
    pub focused_pane: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub tabs: Vec<TabSnapshot>,
    pub active_tab: String,
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
