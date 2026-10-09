//! Workspace state: tabs, split trees, focus. This is the single source of
//! truth; the frontend renders `WorkspaceSnapshot`s and mutates only through
//! commands that call these methods.

use std::collections::HashMap;

use cmux_protocol::{Direction, Node, SplitDir, TabSnapshot, WorkspaceSnapshot};

use crate::layout;

#[derive(Debug, Default, Clone)]
pub struct PaneMeta {
    pub cwd: Option<String>,
    /// True once the shell has reported its directory with OSC 7. That
    /// beats anything the intel poller can read from the OS and must not
    /// be overwritten by it: PowerShell's `cd` moves its own location but
    /// never the *process* working directory the poller sees, so on
    /// Windows the two disagree permanently.
    pub cwd_from_shell: bool,
    /// Set by OSC 0/2 title sequences.
    pub title: Option<String>,
    /// Command pane: the PTY runs this command directly instead of an
    /// interactive shell. Persists across respawns (keypress = rerun).
    pub command: Option<String>,
    /// Git branch + repo root of `cwd` (intel poller).
    pub branch: Option<String>,
    pub repo_root: Option<String>,
    /// Listening TCP ports of the pane's process tree (intel poller).
    pub ports: Vec<u16>,
    /// Browser pane: current URL of its child webview.
    pub browser_url: Option<String>,
    /// The AI agent session running in this pane, captured via hooks for
    /// resume-on-restore: "claude:<session>" for one running in the pane,
    /// "claude-bg:<job>" for a daemon session the pane attaches.
    pub agent_session: Option<String>,
    /// True once the poller has seen that session's process in the pane
    /// this run. When it then disappears the session was exited, and is
    /// forgotten so a restart does not bring back a conversation you left.
    pub agent_live: bool,
    /// Typed into the pane's shell once it first prints, then cleared: how
    /// a restored pane resumes its agent session without a keypress.
    pub startup_input: Option<String>,
    /// The `agentRoles` entry this pane's agent was started with. Kept
    /// until the agent exits, and across restarts so a resume gets the
    /// role's model back.
    pub agent_role: Option<String>,
    /// Started as an agent (`mira agent new`, the wall, the palette), with
    /// or without a role. It makes the pane an agent from the moment it
    /// opens, before Claude has started and a hook has recorded a session —
    /// without it a role-less agent first appeared as a plain tab and moved
    /// to the agents a second later. Cleared, like the role, once Claude
    /// exits; not persisted, since a restart has the session to go by.
    pub agent_launched: bool,
    /// When this run started (or resumed) the pane's agent, until its
    /// `claude` is first seen running; see `agents::forget_agent`.
    pub agent_started_at: Option<std::time::Instant>,
    /// The `--model` the poller saw the pane's `claude` started with.
    pub agent_model: Option<String>,
    /// What the agent's hooks last said it is doing (not persisted: a
    /// restarted app has not heard from it yet).
    pub agent_status: Option<cmux_protocol::AgentStatus>,
    /// The last Notification message from the agent's hooks.
    pub agent_message: Option<String>,
    /// Unix millis of the last `agent_status` change.
    pub agent_since_ms: Option<u64>,
    /// Agent-wall pane: draws the agents of one tab in a grid.
    pub agent_wall: bool,
    /// The tab an agent-wall pane shows the agents of: the tab it was
    /// opened from (see `Workspace::wall_target`).
    pub agent_wall_tab: Option<String>,
    /// Remote pane: the ssh host spec its PTY connects to (`ssh -tt <spec>`).
    pub remote_host: Option<String>,
    /// Diff pane: the repository root its diff is taken in. Paired with
    /// `diff_spec` — both set, or neither.
    pub diff_repo: Option<String>,
    /// Diff pane: "worktree", "staged", or a revspec.
    pub diff_spec: Option<String>,
    /// What's New pane: the version whose release notes it shows.
    pub whats_new: Option<String>,
    /// What's New pane: the version the user came from, when the pane
    /// covers every release after it up to `whats_new` rather than one.
    pub whats_new_since: Option<String>,
    /// Graph pane: the repository root whose commits it draws.
    pub graph_repo: Option<String>,
    /// Graph pane: the diff pane it opened, retargeted on the next click
    /// rather than piling up a pane per commit.
    pub graph_diff_pane: Option<String>,
}

impl PaneMeta {
    /// Runs a Claude Code agent: one that recorded a session, or was
    /// started with a role and has not exited.
    pub fn is_agent(&self) -> bool {
        self.agent_session.is_some() || self.agent_role.is_some() || self.agent_launched
    }
}

#[derive(Debug)]
pub struct Tab {
    pub id: String,
    /// Explicit user rename; derived title otherwise.
    pub title: Option<String>,
    pub root: Node,
    pub focused: String,
    /// One pane shown alone, filling the tab — how the sidebar shows an
    /// agent by itself. Not persisted; cleared by anything that is about
    /// the tab as a whole (see `zoom_pane`).
    pub zoomed: Option<String>,
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
                zoomed: None,
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

    /// Like [`Self::new_tab`], placed the same way, but leaves the active
    /// tab on screen: an agent started from the wall opens behind it, so the
    /// wall never hides and hands every agent's terminal back for a moment.
    pub fn new_background_tab(&mut self) -> (String, String) {
        let (tab, pane) = Tab::new();
        let id = tab.id.clone();
        let at = (self.active + 1).min(self.tabs.len());
        self.tabs.insert(at, tab);
        (id, pane)
    }

    /// Adds a tab at the end for an existing pane id, leaving the active tab
    /// alone: how a session holder no saved pane names gets a home.
    pub fn adopt_pane(&mut self, pane: &str) -> String {
        let tab = Tab {
            id: new_id(),
            title: None,
            root: Node::Leaf {
                pane_id: pane.to_string(),
            },
            focused: pane.to_string(),
            zoomed: None,
        };
        let id = tab.id.clone();
        self.tabs.push(tab);
        id
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

    /// Activates a tab, showing all of it: going to a tab, by its row or
    /// a shortcut, is asking for the tab, so a zoomed pane is let go.
    pub fn set_active_tab(&mut self, tab_id: &str) -> bool {
        if let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) {
            self.active = idx;
            self.tabs[idx].zoomed = None;
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
            tab.zoomed = None;
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
                tab.zoomed = None;
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
        let tab = &mut self.tabs[idx];
        // Focus elsewhere in a zoomed tab shows the tab again; clicking the
        // zoomed pane itself keeps it alone.
        if tab.zoomed.as_deref().is_some_and(|z| z != pane) {
            tab.zoomed = None;
        }
        tab.focused = pane.to_string();
        true
    }

    /// The tab a wall opened from `source` shows: `source`'s own tab —
    /// unless that tab holds a wall itself (opening the wall from the
    /// wall), which keeps showing `current`.
    pub fn wall_target(
        &self,
        source: &str,
        meta: &HashMap<String, PaneMeta>,
        current: Option<&str>,
    ) -> Option<String> {
        let tab = self.tabs.iter().find(|t| layout::contains(&t.root, source))?;
        let is_wall_tab = layout::pane_ids(&tab.root)
            .iter()
            .any(|p| meta.get(p).is_some_and(|m| m.agent_wall));
        if is_wall_tab {
            current.map(str::to_string)
        } else {
            Some(tab.id.clone())
        }
    }

    /// Shows `pane` alone, filling its tab, and focuses it (activating the
    /// tab). The other panes keep running and keep their size; they are
    /// only hidden.
    pub fn zoom_pane(&mut self, pane: &str) -> bool {
        if !self.focus_pane(pane) {
            return false;
        }
        self.tabs[self.active].zoomed = Some(pane.to_string());
        true
    }

    /// Shows the whole of the tab holding `pane` again.
    pub fn unzoom(&mut self, pane: &str) -> bool {
        match self.tab_of_pane_mut(pane) {
            Some(tab) if tab.zoomed.is_some() => {
                tab.zoomed = None;
                true
            }
            _ => false,
        }
    }

    /// Moves focus directionally within the active tab.
    pub fn focus_direction(&mut self, direction: Direction) -> Option<String> {
        let tab = &mut self.tabs[self.active];
        let next = layout::neighbor(&tab.root, &tab.focused, direction)?;
        tab.focused = next.clone();
        tab.zoomed = None;
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
                    // The wall runs no shell: nothing would ever title it.
                    let wall_title = pane_meta
                        .filter(|m| m.agent_wall)
                        .map(|_| "Agents".to_string());
                    let title = t
                        .title
                        .clone()
                        .or(wall_title)
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
                        branch: meta.get(&t.focused).and_then(|m| m.branch.clone()),
                        pr: None,
                        ports: {
                            let mut ports: Vec<u16> = layout::pane_ids(&t.root)
                                .iter()
                                .filter_map(|p| meta.get(p))
                                .flat_map(|m| m.ports.iter().copied())
                                .collect();
                            ports.sort_unstable();
                            ports.dedup();
                            ports
                        },
                        zoomed_pane: t.zoomed.clone(),
                    }
                })
                .collect(),
            active_tab: self.tabs[self.active].id.clone(),
            unread_panes: Vec::new(),
            agent_panes: Vec::new(),
            browser_panes: Vec::new(),
            remote_panes: Vec::new(),
            diff_panes: Vec::new(),
            whats_new_panes: Vec::new(),
            graph_panes: Vec::new(),
            agent_wall_panes: Vec::new(),
            agents: Vec::new(),
        }
    }
}

/// ConPTY seeds a console's title with the shell's own image path
/// (`C:\Program Files\PowerShell\7\pwsh.exe`) and PowerShell never replaces
/// it, so honoring it pins every Windows tab to the same useless string for
/// the session. Fall back to the directory name in that case; a program
/// that sets a real title (`vim: main.rs`, `user@host:~/src`) still wins.
pub fn is_meaningful_title(title: &str) -> bool {
    !title.trim().to_ascii_lowercase().ends_with(".exe")
}

fn display_dir(path: &str) -> String {
    let home = crate::config::home_dir();
    if home.as_deref() == Some(path) {
        return "~".to_string();
    }
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn abbreviate_home(path: &str) -> String {
    if let Some(home) = crate::config::home_dir() {
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
    fn background_tab_opens_beside_the_active_one_and_leaves_it_active() {
        let mut ws = Workspace::default();
        let (_, _) = ws.new_tab();
        let (_, _) = ws.new_tab();
        ws.active = 1;
        let before = ws.tabs[1].id.clone();
        let (id, pane) = ws.new_background_tab();
        assert_eq!(ws.tabs[ws.active].id, before, "the active tab stays on screen");
        assert_eq!(ws.tabs[2].id, id, "placed where new_tab would put it");
        assert!(layout::contains(&ws.tabs[2].root, &pane));
    }

    #[test]
    fn a_zoomed_pane_shows_alone_until_the_tab_is_asked_for() {
        let mut ws = Workspace::default();
        let claude = ws.focused_pane();
        let server = ws.split_pane(&claude, SplitDir::Row).unwrap();
        let tab = ws.tabs[0].id.clone();
        let (_, other) = ws.new_tab();

        // Zooming activates the tab, focuses the pane, and shows it alone.
        assert!(ws.zoom_pane(&claude));
        assert_eq!(ws.active, 0);
        assert_eq!(ws.focused_pane(), claude);
        let zoomed = |ws: &Workspace| ws.snapshot(&HashMap::new()).tabs[0].zoomed_pane.clone();
        assert_eq!(zoomed(&ws), Some(claude.clone()));

        // Clicking the zoomed pane itself, or another tab, keeps it.
        assert!(ws.focus_pane(&claude));
        assert!(ws.focus_pane(&other));
        assert_eq!(zoomed(&ws), Some(claude.clone()));

        // Going to the tab shows all of it.
        ws.set_active_tab(&tab);
        assert_eq!(zoomed(&ws), None);

        // So does focusing another of its panes, splitting, or closing one.
        ws.zoom_pane(&claude);
        ws.focus_pane(&server);
        assert_eq!(zoomed(&ws), None);
        ws.zoom_pane(&claude);
        let third = ws.split_pane(&server, SplitDir::Column).unwrap();
        assert_eq!(zoomed(&ws), None);
        ws.zoom_pane(&claude);
        ws.close_pane(&third);
        assert_eq!(zoomed(&ws), None);
        ws.zoom_pane(&claude);
        assert!(ws.unzoom(&claude));
        assert_eq!(zoomed(&ws), None);
        assert!(!ws.unzoom(&claude), "nothing to let go of");
    }

    #[test]
    fn a_wall_shows_the_tab_it_was_opened_from() {
        let mut ws = Workspace::default();
        let work = ws.focused_pane();
        let first = ws.tabs[0].id.clone();
        let (second, other) = ws.new_tab();
        let (_, wall) = ws.new_tab();
        let mut meta = HashMap::new();
        meta.insert(wall.clone(), PaneMeta { agent_wall: true, ..Default::default() });

        assert_eq!(ws.wall_target(&work, &meta, None), Some(first.clone()));
        // Opened again from another tab, it shows that one.
        assert_eq!(ws.wall_target(&other, &meta, Some(&first)), Some(second));
        // Opened from the wall itself, it keeps what it shows.
        assert_eq!(ws.wall_target(&wall, &meta, Some(&first)), Some(first));
        assert_eq!(ws.wall_target(&wall, &meta, None), None);
        assert_eq!(ws.wall_target("gone", &meta, None), None);
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
    fn conpty_image_path_titles_are_rejected() {
        // What ConPTY seeds every Windows console with.
        assert!(!is_meaningful_title(r"C:\Program Files\PowerShell\7\pwsh.exe"));
        assert!(!is_meaningful_title("C:\\Windows\\System32\\cmd.exe "));
        // What a program actually setting a title looks like.
        assert!(is_meaningful_title("vim: main.rs"));
        assert!(is_meaningful_title("yoan@box:~/src"));
    }

    #[test]
    fn an_adopted_pane_gets_a_tab_at_the_end_without_taking_focus() {
        let mut ws = Workspace::default();
        let (_, second) = ws.new_tab();
        let active = ws.active;
        ws.adopt_pane("orphan-1");
        assert_eq!(ws.active, active);
        assert_eq!(ws.focused_pane(), second);
        assert_eq!(ws.all_pane_ids().last().map(String::as_str), Some("orphan-1"));
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
