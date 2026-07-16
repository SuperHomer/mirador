//! Workspace state: tabs, split trees, focus. This is the single source of
//! truth; the frontend renders `WorkspaceSnapshot`s and mutates only through
//! commands that call these methods.

use std::collections::HashMap;

use cmux_protocol::{Direction, Node, SplitDir, TabSnapshot, WorkspaceSnapshot};

use crate::layout;

#[derive(Debug, Default, Clone)]
pub struct PaneMeta {
    pub cwd: Option<String>,
    /// Set by OSC 0/2 title sequences.
    pub title: Option<String>,
    /// Command pane: the PTY runs this command directly instead of an
    /// interactive shell. Persists across respawns (keypress = rerun).
    pub command: Option<String>,
}

#[derive(Debug)]
pub struct Tab {
    pub id: String,
    /// Explicit user rename; derived title otherwise.
    pub title: Option<String>,
    pub root: Node,
    pub focused: String,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl Tab {
    fn new() -> (Self, String) {
        let pane = new_id();
        (
            Self {
                id: new_id(),
                title: None,
                root: Node::Leaf {
                    pane_id: pane.clone(),
                },
                focused: pane.clone(),
            },
            pane,
        )
    }
}

#[derive(Debug)]
pub struct Workspace {
    pub tabs: Vec<Tab>,
    pub active: usize,
}

impl Default for Workspace {
    fn default() -> Self {
        let (tab, _) = Tab::new();
        Self {
            tabs: vec![tab],
            active: 0,
        }
    }
}

impl Workspace {
    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn focused_pane(&self) -> String {
        self.active_tab().focused.clone()
    }

    fn tab_of_pane_mut(&mut self, pane: &str) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| layout::contains(&t.root, pane))
    }

    /// Creates a tab (with one fresh pane) after the active one and
    /// activates it. Returns (tab_id, pane_id).
    pub fn new_tab(&mut self) -> (String, String) {
        let (tab, pane) = Tab::new();
        let id = tab.id.clone();
        self.active = (self.active + 1).min(self.tabs.len());
        self.tabs.insert(self.active, tab);
        (id, pane)
    }

    /// Removes a tab, returning the pane ids to kill. Always keeps at least
    /// one tab (a fresh one is created if the last was closed).
    pub fn close_tab(&mut self, tab_id: &str) -> Vec<String> {
        let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return Vec::new();
        };
        let tab = self.tabs.remove(idx);
        let panes = layout::pane_ids(&tab.root);
        if self.tabs.is_empty() {
            let (tab, _) = Tab::new();
            self.tabs.push(tab);
            self.active = 0;
        } else if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        } else if idx < self.active {
            self.active -= 1;
        }
        panes
    }

    pub fn set_active_tab(&mut self, tab_id: &str) -> bool {
        if let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) {
            self.active = idx;
            true
        } else {
            false
        }
    }

    pub fn rename_tab(&mut self, tab_id: &str, title: &str) -> bool {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            tab.title = if title.trim().is_empty() {
                None
            } else {
                Some(title.trim().to_string())
            };
            true
        } else {
            false
        }
    }

    pub fn move_tab(&mut self, tab_id: &str, to: usize) -> bool {
        let Some(from) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return false;
        };
        let to = to.min(self.tabs.len() - 1);
        let active_id = self.tabs[self.active].id.clone();
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
        self.active = self.tabs.iter().position(|t| t.id == active_id).unwrap();
        true
    }

    /// Splits the pane, focusing the new pane. Returns its id.
    pub fn split_pane(&mut self, pane: &str, dir: SplitDir) -> Option<String> {
        let tab = self.tab_of_pane_mut(pane)?;
        let new_pane = new_id();
        if layout::split(&mut tab.root, pane, dir, &new_pane) {
            tab.focused = new_pane.clone();
            Some(new_pane)
        } else {
            None
        }
    }

    /// Removes the pane (killing list returned). Focus moves to a sibling;
    /// an emptied tab is removed (workspace never ends up tabless).
    pub fn close_pane(&mut self, pane: &str) -> Vec<String> {
        let Some(tab) = self.tab_of_pane_mut(pane) else {
            return Vec::new();
        };
        let tab_id = tab.id.clone();
        match layout::remove(&mut tab.root, pane) {
            layout::RemoveOutcome::BecameEmpty => self.close_tab(&tab_id),
            layout::RemoveOutcome::Removed => {
                if tab.focused == pane {
                    tab.focused = layout::pane_ids(&tab.root)
                        .first()
                        .cloned()
                        .unwrap_or_default();
                }
                vec![pane.to_string()]
            }
            layout::RemoveOutcome::NotFound => Vec::new(),
        }
    }

    /// Focuses a pane, also activating its tab.
    pub fn focus_pane(&mut self, pane: &str) -> bool {
        let Some(idx) = self
            .tabs
            .iter()
            .position(|t| layout::contains(&t.root, pane))
        else {
            return false;
        };
        self.active = idx;
        self.tabs[idx].focused = pane.to_string();
        true
    }

    /// Moves focus directionally within the active tab.
    pub fn focus_direction(&mut self, direction: Direction) -> Option<String> {
        let tab = &mut self.tabs[self.active];
        let next = layout::neighbor(&tab.root, &tab.focused, direction)?;
        tab.focused = next.clone();
        Some(next)
    }

    pub fn set_split_ratios(&mut self, tab_id: &str, path: &[usize], ratios: Vec<f32>) -> bool {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
            layout::set_ratios(&mut tab.root, path, ratios)
        } else {
            false
        }
    }

    pub fn all_pane_ids(&self) -> Vec<String> {
        self.tabs
            .iter()
            .flat_map(|t| layout::pane_ids(&t.root))
            .collect()
    }

    pub fn snapshot(&self, meta: &HashMap<String, PaneMeta>) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            tabs: self
                .tabs
                .iter()
                .map(|t| {
                    let pane_meta = meta.get(&t.focused);
                    let cwd = pane_meta.and_then(|m| m.cwd.clone());
                    let osc_title = pane_meta.and_then(|m| m.title.clone());
                    let title = t
                        .title
                        .clone()
                        .or(osc_title)
                        .unwrap_or_else(|| {
                            cwd.as_deref()
                                .map(display_dir)
                                .unwrap_or_else(|| "shell".to_string())
                        });
                    TabSnapshot {
                        id: t.id.clone(),
                        title,
                        cwd: cwd.clone().map(|c| abbreviate_home(&c)),
                        root: t.root.clone(),
                        focused_pane: t.focused.clone(),
                        unread: 0,
                        last_notification: None,
                    }
                })
                .collect(),
            active_tab: self.tabs[self.active].id.clone(),
            unread_panes: Vec::new(),
            agent_panes: Vec::new(),
        }
    }
}

fn display_dir(path: &str) -> String {
    let home = std::env::var("HOME").ok();
    if home.as_deref() == Some(path) {
        return "~".to_string();
    }
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn abbreviate_home(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if let Some(rest) = path.strip_prefix(&home) {
            return format!("~{rest}");
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tab_inserts_after_active_and_activates() {
        let mut ws = Workspace::default();
        let first = ws.tabs[0].id.clone();
        let (second, _) = ws.new_tab();
        assert_eq!(ws.active_tab().id, second);
        ws.set_active_tab(&first);
        let (third, _) = ws.new_tab();
        assert_eq!(ws.tabs[1].id, third);
    }

    #[test]
    fn close_last_tab_recreates_one() {
        let mut ws = Workspace::default();
        let id = ws.tabs[0].id.clone();
        let killed = ws.close_tab(&id);
        assert_eq!(killed.len(), 1);
        assert_eq!(ws.tabs.len(), 1);
        assert_ne!(ws.tabs[0].id, id);
    }

    #[test]
    fn close_pane_collapses_and_refocuses() {
        let mut ws = Workspace::default();
        let a = ws.focused_pane();
        let b = ws.split_pane(&a, SplitDir::Row).unwrap();
        assert_eq!(ws.focused_pane(), b);
        let killed = ws.close_pane(&b);
        assert_eq!(killed, vec![b]);
        assert_eq!(ws.focused_pane(), a);
    }

    #[test]
    fn close_last_pane_closes_tab() {
        let mut ws = Workspace::default();
        let (_, pane2) = ws.new_tab();
        assert_eq!(ws.tabs.len(), 2);
        let killed = ws.close_pane(&pane2);
        assert_eq!(killed, vec![pane2]);
        assert_eq!(ws.tabs.len(), 1);
    }

    #[test]
    fn focus_pane_activates_owning_tab() {
        let mut ws = Workspace::default();
        let a = ws.focused_pane();
        ws.new_tab();
        assert_ne!(ws.focused_pane(), a);
        assert!(ws.focus_pane(&a));
        assert_eq!(ws.focused_pane(), a);
        assert_eq!(ws.active, 0);
    }

    #[test]
    fn snapshot_titles_derive_from_cwd() {
        let ws = Workspace::default();
        let pane = ws.focused_pane();
        let mut meta = HashMap::new();
        meta.insert(
            pane,
            PaneMeta {
                cwd: Some("/tmp/project".to_string()),
                ..Default::default()
            },
        );
        let snap = ws.snapshot(&meta);
        assert_eq!(snap.tabs[0].title, "project");
    }
}
