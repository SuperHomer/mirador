//! Workspace state: tabs, split trees, focus. This is the single source of
//! truth; the frontend renders `WorkspaceSnapshot`s and mutates only through
//! commands that call these methods.

use std::collections::HashMap;

use cmux_protocol::{
    BoardPoint, BoardRect, Camera, CardRect, Direction, Node, SplitDir, TabSnapshot, View,
    WorkspaceSnapshot,
};

use crate::{board, layout};

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
    /// The settings file holding the role's Claude Code to its MCP servers,
    /// plugins, skills and tools (`roles::write_settings`). Kept, like the
    /// role, so a resume passes it again.
    pub agent_settings: Option<String>,
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
    /// The Claude Code session's transcript, as its hooks report it: where
    /// its progress and its subagents are read from. Not persisted; the
    /// next hook reports it again.
    pub agent_transcript: Option<std::path::PathBuf>,
    /// The agent program the poller saw running in the pane — Claude Code,
    /// Codex, OpenCode, …: what makes a pane whose agent has no hooks an
    /// agent at all. Not persisted; a restart sees the process again.
    pub agent_harness: Option<cmux_protocol::Harness>,
    /// The pid of the agent process the poller saw on the pane's terminal.
    pub agent_pid: Option<u32>,
    /// The Claude Code session's name, from its session file (what
    /// `claude agents` lists): one given with `--name` or `/rename`, or
    /// one Claude Code made up.
    pub agent_name: Option<String>,
    /// Busy or idle, as that file says: the status of an agent whose hooks
    /// have not reported one (or are not installed).
    pub agent_session_status: Option<cmux_protocol::AgentStatus>,
    /// The `--model` the poller saw the pane's agent started with.
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
    /// Agent-editor pane: the role, way and team files its directory sees,
    /// for editing.
    pub agent_editor: bool,
    /// Task-board pane: the cards of one tab's agents, or of every tab's.
    pub task_board: bool,
    /// The tab a task-board pane shows the cards of: the tab it was opened
    /// from.
    pub task_board_tab: Option<String>,
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
    /// Runs a coding agent: one seen in its process table, or a Claude Code
    /// one that recorded a session or was started (with or without a role)
    /// and has not exited.
    pub fn is_agent(&self) -> bool {
        self.agent_harness.is_some() || self.claude_marked()
    }

    /// What it runs. The session, role and launch mark are all Claude
    /// Code's, so an agent known only by them is a Claude Code one.
    pub fn harness(&self) -> Option<cmux_protocol::Harness> {
        self.agent_harness
            .or_else(|| self.claude_marked().then_some(cmux_protocol::Harness::Claude))
    }

    fn claude_marked(&self) -> bool {
        self.agent_session.is_some() || self.agent_role.is_some() || self.agent_launched
    }

    /// Forgets everything that makes it an agent, and what it was doing.
    pub fn clear_agent(&mut self) {
        self.agent_live = false;
        self.agent_started_at = None;
        self.agent_session = None;
        self.agent_role = None;
        self.agent_settings = None;
        self.agent_launched = false;
        self.agent_harness = None;
        self.agent_transcript = None;
        self.agent_pid = None;
        self.agent_name = None;
        self.agent_session_status = None;
        self.agent_model = None;
        self.agent_status = None;
        self.agent_message = None;
        self.agent_since_ms = None;
    }

    /// Applies what the process table shows on the pane's terminal now —
    /// `running` is its agent, if any — and says whether anything a
    /// snapshot shows changed. `auto_resume` is
    /// [`crate::agents::AUTO_RESUME`], a parameter so both platforms are
    /// tested everywhere.
    ///
    /// An agent showing up makes the pane one, whatever started it. A
    /// different agent than the pane had means the old one exited, and its
    /// Claude session goes with it: left in place, the next launch would
    /// type `claude --resume` into a pane you had moved on to Codex in.
    /// No agent at all is [`crate::agents::forget_agent`]'s call.
    pub fn observe_agent(
        &mut self,
        running: Option<&crate::agents::RunningAgent>,
        auto_resume: bool,
    ) -> bool {
        let Some(run) = running else {
            if !self.is_agent() {
                return false;
            }
            let forget = crate::agents::forget_agent(
                false,
                self.agent_live,
                self.startup_input.is_some(),
                // Windows lists no processes: there "never seen" says
                // nothing, and the grace period must not run.
                self.agent_started_at
                    .filter(|_| auto_resume)
                    .map(|t| t.elapsed()),
            );
            if forget {
                self.clear_agent();
            }
            return forget;
        };
        let mut changed = false;
        if self.harness().is_some_and(|h| h != run.harness) {
            self.clear_agent();
            changed = true;
        }
        if self.agent_harness != Some(run.harness) {
            self.agent_harness = Some(run.harness);
            changed = true;
        }
        self.agent_live = true;
        self.agent_started_at = None;
        self.agent_pid = Some(run.pid);
        // The model it was started on; a role's is already there, and agrees.
        let model = crate::agents::model_flag(&run.args);
        if model.is_some() && self.agent_model != model {
            self.agent_model = model;
            changed = true;
        }
        changed
    }

    /// Takes the name and status from the Claude Code session file found
    /// for this pane (`agents::find_claude_session`), and says whether
    /// either changed. None found leaves the last ones: a file being
    /// rewritten can be missed for a tick.
    pub fn apply_claude_session(&mut self, found: Option<&crate::agents::ClaudeSession>) -> bool {
        let Some(found) = found else { return false };
        let changed =
            self.agent_name != found.name || self.agent_session_status != found.status;
        self.agent_name = found.name.clone();
        self.agent_session_status = found.status;
        changed
    }

    /// Applies the poller's read of the pane's directory and its git
    /// branch (`repo root`, `branch`), and says whether anything changed.
    /// `from_os`: the directory was read from the OS, because the shell had
    /// not reported one with OSC 7 when the poll began. If it has since,
    /// the shell's stands, and so does the branch worked out for it: the
    /// poll was made for a directory that no longer counts, and the next
    /// one is made for the right one.
    pub fn observe_cwd(
        &mut self,
        cwd: Option<String>,
        from_os: bool,
        git: Option<(String, String)>,
    ) -> bool {
        if from_os && self.cwd_from_shell {
            return false;
        }
        let mut changed = false;
        if let Some(cwd) = cwd {
            if self.cwd.as_deref() != Some(cwd.as_str()) {
                self.cwd = Some(cwd);
                changed = true;
            }
        }
        let (root, branch) = git.unzip();
        if self.branch != branch || self.repo_root != root {
            self.branch = branch;
            self.repo_root = root;
            changed = true;
        }
        changed
    }

    /// Whether its activity can be read: a Claude Code agent whose hooks
    /// reported its transcript.
    pub fn has_activity(&self) -> bool {
        self.harness() == Some(cmux_protocol::Harness::Claude) && self.agent_transcript.is_some()
    }

    /// What the agent is doing: what its hooks said, else what Claude
    /// Code's session file says.
    pub fn status(&self) -> Option<cmux_protocol::AgentStatus> {
        self.agent_status.or(self.agent_session_status)
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
    /// The team running in this tab, if one was started here: who is in
    /// which pane. Persisted, so a team is still a team after a restart.
    pub team: Option<cmux_protocol::TeamRun>,
    /// Where the tab's frame sits on the board.
    pub frame: BoardPoint,
    /// Each pane's card on the board, relative to `frame`. Kept for every
    /// pane in either view, so the board is ready whenever it is shown.
    pub cards: board::Cards,
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl Tab {
    fn new() -> (Self, String) {
        let pane = new_id();
        (Self::with_root(Node::Leaf { pane_id: pane.clone() }), pane)
    }

    /// A tab laid out as `root`, its first pane focused, its cards its
    /// splits. Its frame is placed by the workspace that takes it in.
    fn with_root(root: Node) -> Self {
        let focused = layout::pane_ids(&root).into_iter().next().unwrap_or_default();
        Self {
            id: new_id(),
            title: None,
            cards: board::default_cards(&root),
            root,
            focused,
            zoomed: None,
            team: None,
            frame: BoardPoint::default(),
        }
    }

    /// The tab's frame on the board.
    pub fn frame_rect(&self) -> BoardRect {
        board::frame_rect(self.frame, &self.cards)
    }
}

#[derive(Debug)]
pub struct Workspace {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub view: View,
    pub camera: Camera,
    /// The board's size on screen, in pixels, as the app last said: with
    /// the camera, where the board is being looked at. Not saved — unknown
    /// until the app has drawn the board.
    pub viewport: Option<(f64, f64)>,
}

impl Default for Workspace {
    fn default() -> Self {
        let (tab, _) = Tab::new();
        Self {
            tabs: vec![tab],
            active: 0,
            view: View::default(),
            camera: Camera::default(),
            viewport: None,
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

    /// Gives a tab just taken in a frame of its own on the board, never on
    /// top of one already there: with the board shown, where it is being
    /// looked at (`board::frame_spot`) — a tab opened there is one to see —
    /// and otherwise right of every other, wherever it sits in the tab order.
    fn place_frame(&mut self, tab_id: &str) {
        let others = self.other_frames(tab_id);
        let looked_at = match (self.view, self.viewport) {
            (View::Board, Some((w, h))) => Some(BoardPoint {
                x: self.camera.x + w / self.camera.zoom / 2.0,
                y: self.camera.y + h / self.camera.zoom / 2.0,
            }),
            _ => None,
        };
        let Some(tab) = self.tab_mut(tab_id) else { return };
        tab.frame = match looked_at {
            Some(centre) => {
                let size = tab.frame_rect();
                board::frame_spot(size.w, size.h, centre, &others)
            }
            None => board::next_frame_origin(others),
        };
    }

    /// Creates a tab (with one fresh pane) after the active one and
    /// activates it. Returns (tab_id, pane_id).
    pub fn new_tab(&mut self) -> (String, String) {
        let (tab, pane) = Tab::new();
        let id = tab.id.clone();
        self.active = (self.active + 1).min(self.tabs.len());
        self.tabs.insert(self.active, tab);
        self.place_frame(&id);
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
        self.place_frame(&id);
        (id, pane)
    }

    /// Opens a team's tab after the active one, its panes laid out as
    /// `root`, the first focused. Returns the tab id.
    pub fn new_team_tab(&mut self, root: Node, team: cmux_protocol::TeamRun, title: &str) -> String {
        let tab = Tab {
            title: Some(title.to_string()),
            team: Some(team),
            ..Tab::with_root(root)
        };
        let id = tab.id.clone();
        self.active = (self.active + 1).min(self.tabs.len());
        self.tabs.insert(self.active, tab);
        self.place_frame(&id);
        id
    }

    /// New pane ids, for a layout built before its tab exists.
    pub fn fresh_pane_ids(n: usize) -> Vec<String> {
        (0..n).map(|_| new_id()).collect()
    }

    /// The tab holding `pane`, for reading.
    pub fn tab_of_pane(&self, pane: &str) -> Option<&Tab> {
        self.tabs.iter().find(|t| layout::contains(&t.root, pane))
    }

    /// The tab with this id, for changing its team.
    pub fn tab_mut(&mut self, id: &str) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    /// Adds a tab at the end for an existing pane id, leaving the active tab
    /// alone: how a session holder no saved pane names gets a home.
    pub fn adopt_pane(&mut self, pane: &str) -> String {
        let tab = Tab::with_root(Node::Leaf {
            pane_id: pane.to_string(),
        });
        let id = tab.id.clone();
        self.tabs.push(tab);
        self.place_frame(&id);
        id
    }

    /// Removes a tab, returning the pane ids to kill. Always keeps at least
    /// one tab (a fresh one is created if the last was closed).
    pub fn close_tab(&mut self, tab_id: &str) -> Vec<String> {
        let Some(idx) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return Vec::new();
        };
        let closed = self.tabs.remove(idx);
        let panes = layout::pane_ids(&closed.root);
        if self.tabs.is_empty() {
            // Where the last frame was, which the board is showing.
            let (tab, _) = Tab::new();
            self.tabs.push(Tab {
                frame: closed.frame,
                ..tab
            });
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
            match board::card_spot(&tab.cards, pane, dir) {
                Some(card) => {
                    tab.cards.insert(new_pane.clone(), card);
                }
                None => board::settle(&tab.root, &mut tab.cards),
            }
            let tab_id = tab.id.clone();
            self.push_frames_clear(&tab_id);
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
                tab.cards.remove(pane);
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

    /// Shows the workspace as tabs or as the board. False when it already was.
    pub fn set_view(&mut self, view: View) -> bool {
        std::mem::replace(&mut self.view, view) != view
    }

    /// The frames of every tab but `tab_id`'s.
    fn other_frames(&self, tab_id: &str) -> Vec<BoardRect> {
        self.tabs
            .iter()
            .filter(|t| t.id != tab_id)
            .map(Tab::frame_rect)
            .collect()
    }

    /// Keeps frames apart after `tab_id`'s frame grew: frames it reaches
    /// are pushed clear (`board::push_clear`). Every change that can grow a
    /// frame ends here, so no two frames — and so no two cards — overlap.
    pub(crate) fn push_frames_clear(&mut self, tab_id: &str) {
        let Some(grown) = self.tabs.iter().position(|t| t.id == tab_id) else {
            return;
        };
        let mut frames: Vec<BoardRect> = self.tabs.iter().map(Tab::frame_rect).collect();
        board::push_clear(&mut frames, grown);
        for (tab, frame) in self.tabs.iter_mut().zip(frames) {
            tab.frame.x = frame.x;
        }
    }

    /// Moves or resizes `pane`'s card, if no other card of its frame is
    /// there (`board::card_fits`); its splits stay as they are. The card's
    /// place afterwards — unchanged when refused — or None for no such pane.
    pub fn set_card_rect(&mut self, pane: &str, rect: BoardRect) -> Option<BoardRect> {
        let tab_id = self.tab_of_pane(pane)?.id.clone();
        let tab = self.tab_mut(&tab_id)?;
        if board::card_fits(&tab.cards, pane, rect) {
            tab.cards.insert(pane.to_string(), rect);
        }
        let placed = tab.cards.get(pane).copied();
        self.push_frames_clear(&tab_id);
        placed
    }

    /// Moves a tab's frame, and its cards with it, if no other frame is
    /// there. Where the frame is afterwards, or None for no such tab.
    pub fn set_frame_position(&mut self, tab_id: &str, origin: BoardPoint) -> Option<BoardPoint> {
        let others = self.other_frames(tab_id);
        let tab = self.tab_mut(tab_id)?;
        if board::frame_fits(&tab.cards, origin, &others) {
            tab.frame = origin;
        }
        Some(tab.frame)
    }

    /// Packs every frame, and their cards with them, in tab order for a
    /// `width` × `height` viewport (`board::arrange`), from the frames'
    /// top-left as it is, so the board stays about where it was.
    pub fn arrange_frames(&mut self, width: f64, height: f64) {
        let frames: Vec<BoardRect> = self.tabs.iter().map(Tab::frame_rect).collect();
        let origin = BoardPoint {
            x: frames.iter().map(|f| f.x).fold(f64::INFINITY, f64::min),
            y: frames.iter().map(|f| f.y).fold(f64::INFINITY, f64::min),
        };
        let sizes: Vec<(f64, f64)> = frames.iter().map(|f| (f.w, f.h)).collect();
        let spots = board::arrange(&sizes, origin, width, height);
        for (tab, spot) in self.tabs.iter_mut().zip(spots) {
            tab.frame = spot;
        }
    }

    /// Whether `pane` is still in a tab. A pane leaves its tab before its
    /// `PaneMeta` is removed, so a writer that checks this under the
    /// workspace lock never brings a closed pane's meta back.
    pub fn has_pane(&self, pane: &str) -> bool {
        self.tabs
            .iter()
            .any(|t| layout::pane_ids(&t.root).iter().any(|p| p == pane))
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
                    // The wall and the board run no shell: nothing would
                    // ever title them.
                    let wall_title = pane_meta.and_then(|m| {
                        m.agent_wall
                            .then(|| "Agents".to_string())
                            .or_else(|| m.task_board.then(|| "Tasks".to_string()))
                    });
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
                        team: t.team.clone(),
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
                        frame: t.frame_rect(),
                        cards: layout::pane_ids(&t.root)
                            .into_iter()
                            .filter_map(|pane| {
                                let rect = *t.cards.get(&pane)?;
                                let title = pane_title(meta.get(&pane));
                                Some(CardRect { pane_id: pane, title, rect })
                            })
                            .collect(),
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
            agent_editor_panes: Vec::new(),
            task_board_panes: Vec::new(),
            agents: Vec::new(),
            view: self.view,
            camera: self.camera,
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

/// What a pane is, in a few words, for its card on the board: what it
/// shows for the panes that are not shells, else what its program titled
/// it, else where it works.
pub fn pane_title(meta: Option<&PaneMeta>) -> String {
    let Some(m) = meta else {
        return "shell".to_string();
    };
    let name = |path: &str| display_dir(path);
    if m.agent_wall {
        "Agents".to_string()
    } else if m.agent_editor {
        "Roles, ways & teams".to_string()
    } else if m.task_board {
        "Tasks".to_string()
    } else if let Some(version) = &m.whats_new {
        format!("What's new in {version}")
    } else if let Some(url) = &m.browser_url {
        url.clone()
    } else if let Some(repo) = &m.diff_repo {
        format!("Diff · {}", name(repo))
    } else if let Some(repo) = &m.graph_repo {
        format!("Graph · {}", name(repo))
    } else if let Some(title) = m.title.as_deref().filter(|t| is_meaningful_title(t)) {
        title.to_string()
    } else if let Some(cwd) = &m.cwd {
        name(cwd)
    } else {
        "shell".to_string()
    }
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

    fn running(harness: cmux_protocol::Harness, args: &str) -> crate::agents::RunningAgent {
        crate::agents::RunningAgent { harness, pid: 42, args: args.into() }
    }

    #[test]
    fn a_directory_the_shell_reported_mid_poll_is_not_overwritten() {
        let mut m = PaneMeta::default();
        let git = || Some(("/repo".to_string(), "main".to_string()));
        assert!(m.observe_cwd(Some("/repo".into()), true, git()));
        assert!(!m.observe_cwd(Some("/repo".into()), true, git()));
        // OSC 7 lands while the poll is reading the OS (on Windows, the
        // launch directory): the shell's directory and branch stay.
        m.cwd = Some("/elsewhere".into());
        m.cwd_from_shell = true;
        m.branch = None;
        m.repo_root = None;
        assert!(!m.observe_cwd(Some("/launch".into()), true, None));
        assert_eq!(m.cwd.as_deref(), Some("/elsewhere"));
        // A poll that began after it took the shell's directory, and works
        // out its branch.
        assert!(m.observe_cwd(Some("/elsewhere".into()), false, Some(("/elsewhere".into(), "dev".into()))));
        assert_eq!(m.branch.as_deref(), Some("dev"));
    }

    #[test]
    fn a_session_file_names_the_agent_and_stands_in_for_hooks() {
        use cmux_protocol::{AgentStatus, Harness};
        let mut m = PaneMeta::default();
        m.observe_agent(Some(&running(Harness::Claude, "claude")), true);
        assert_eq!(m.agent_pid, Some(42));
        let file = crate::agents::ClaudeSession {
            pid: 42,
            name: Some("fix-login".into()),
            status: Some(AgentStatus::Working),
            ..Default::default()
        };
        assert!(m.apply_claude_session(Some(&file)));
        assert!(!m.apply_claude_session(Some(&file)));
        assert!(!m.apply_claude_session(None));
        assert_eq!(m.agent_name.as_deref(), Some("fix-login"));
        assert_eq!(m.status(), Some(AgentStatus::Working));
        // A hook's status is more precise (it knows about permission
        // prompts), and wins.
        m.agent_status = Some(AgentStatus::NeedsYou);
        assert_eq!(m.status(), Some(AgentStatus::NeedsYou));
        // Gone with the agent.
        m.clear_agent();
        assert_eq!((m.agent_name, m.agent_session_status, m.agent_pid), (None, None, None));
    }

    #[test]
    fn any_harness_seen_running_makes_an_agent() {
        use cmux_protocol::Harness;
        let mut m = PaneMeta::default();
        assert!(!m.observe_agent(None, true));
        assert!(!m.is_agent());

        assert!(m.observe_agent(Some(&running(Harness::Codex, "codex -m gpt-5-codex")), true));
        assert!(m.is_agent());
        assert_eq!(m.harness(), Some(Harness::Codex));
        assert_eq!(m.agent_model.as_deref(), Some("gpt-5-codex"));
        // Seen again: nothing new.
        assert!(!m.observe_agent(Some(&running(Harness::Codex, "codex -m gpt-5-codex")), true));
        // Exited: forgotten.
        assert!(m.observe_agent(None, true));
        assert!(!m.is_agent());
        assert_eq!(m.agent_model, None);
    }

    #[test]
    fn a_hooked_claude_pane_is_a_claude_agent_before_it_is_seen() {
        use cmux_protocol::Harness;
        let mut m = PaneMeta { agent_session: Some("claude-fg:s1".into()), ..Default::default() };
        assert_eq!(m.harness(), Some(Harness::Claude));
        m.observe_agent(Some(&running(Harness::Claude, "claude")), true);
        assert_eq!(m.agent_session.as_deref(), Some("claude-fg:s1"));
    }

    #[test]
    fn another_harness_replaces_the_claude_session() {
        use cmux_protocol::Harness;
        let mut m = PaneMeta {
            agent_session: Some("claude-fg:s1".into()),
            agent_role: Some("reviewer".into()),
            agent_model: Some("opus".into()),
            agent_status: Some(cmux_protocol::AgentStatus::Idle),
            ..Default::default()
        };
        assert!(m.observe_agent(Some(&running(Harness::OpenCode, "opencode")), true));
        assert_eq!(m.harness(), Some(Harness::OpenCode));
        // Nothing of the Claude agent is left to resume or show.
        assert_eq!(m.agent_session, None);
        assert_eq!(m.agent_role, None);
        assert_eq!(m.agent_model, None);
        assert_eq!(m.agent_status, None);
    }

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

    /// Every pane of every tab has a card, and no frame covers another.
    fn assert_board_consistent(ws: &Workspace) {
        for t in &ws.tabs {
            let mut panes = layout::pane_ids(&t.root);
            let mut cards: Vec<String> = t.cards.keys().cloned().collect();
            panes.sort();
            cards.sort();
            assert_eq!(panes, cards, "tab {}", t.id);
        }
        let frames: Vec<BoardRect> = ws.tabs.iter().map(Tab::frame_rect).collect();
        for (i, a) in frames.iter().enumerate() {
            for b in &frames[i + 1..] {
                let apart = a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
                assert!(apart, "{a:?} covers {b:?}");
            }
        }
    }

    #[test]
    fn every_pane_has_a_card_however_panes_come_and_go() {
        let mut ws = Workspace::default();
        assert_board_consistent(&ws);
        let a = ws.focused_pane();
        let b = ws.split_pane(&a, SplitDir::Row).unwrap();
        ws.split_pane(&b, SplitDir::Column).unwrap();
        ws.new_tab();
        let first = ws.tabs[0].id.clone();
        ws.set_active_tab(&first);
        // Inserted in the tab order before the second tab, its frame still
        // goes right of every other.
        ws.new_background_tab();
        let panes = Workspace::fresh_pane_ids(2);
        let team = cmux_protocol::TeamRun { name: "t".into(), members: vec![] };
        ws.new_team_tab(crate::teams::layout(&panes, None), team, "t");
        ws.adopt_pane("orphan");
        assert_board_consistent(&ws);
        ws.close_pane(&b);
        assert!(!ws.tabs[0].cards.contains_key(&b));
        assert_board_consistent(&ws);
    }

    #[test]
    fn on_the_board_a_new_frame_goes_where_it_is_looked_at() {
        let mut ws = Workspace { view: View::Board, ..Default::default() };
        // Not drawn yet: where it would go in the tabs view.
        let (first, _) = ws.new_tab();
        let right = ws.tabs[0].frame_rect();
        assert_eq!(ws.tab_mut(&first).unwrap().frame.x, right.x + right.w + board::FRAME_GAP);
        // Looking at an empty stretch far below: there, centred.
        ws.viewport = Some((1000.0, 800.0));
        ws.camera = Camera { x: 0.0, y: 5000.0, zoom: 0.5 };
        let (id, _) = ws.new_tab();
        let frame = ws.tabs.iter().find(|t| t.id == id).unwrap().frame_rect();
        assert_eq!((frame.x + frame.w / 2.0, frame.y + frame.h / 2.0), (1000.0, 5800.0));
        assert_nothing_overlaps(&ws);
        // Again, same view: beside it, not on it.
        ws.new_tab();
        assert_nothing_overlaps(&ws);
    }

    #[test]
    fn a_new_frame_goes_right_of_the_rightmost() {
        let mut ws = Workspace::default();
        let first = ws.tabs[0].id.clone();
        ws.set_frame_position(&first, BoardPoint { x: 5000.0, y: 300.0 });
        let (second, _) = ws.new_tab();
        let tab = ws.tabs.iter().find(|t| t.id == second).unwrap();
        let right = ws.tabs[0].frame_rect();
        assert_eq!(tab.frame, BoardPoint { x: right.x + right.w + board::FRAME_GAP, y: 0.0 });
    }

    #[test]
    fn a_split_adds_a_card_in_its_frame_and_moves_none() {
        let mut ws = Workspace::default();
        let a = ws.focused_pane();
        // Moved down and right: the frame has room above and left of it.
        let moved = BoardRect { x: 600.0, y: 400.0, w: 500.0, h: 250.0 };
        assert_eq!(ws.set_card_rect(&a, moved), Some(moved));
        let b = ws.split_pane(&a, SplitDir::Row).unwrap();
        let cards = &ws.tabs[0].cards;
        assert_eq!(cards[&a], moved, "the source stays where it was put");
        let inner = (board::FRAME_PADDING + board::CARD_GAP / 2.0, board::FRAME_HEADER + board::CARD_GAP / 2.0);
        assert_eq!(cards[&b], BoardRect { x: inner.0, y: inner.1, ..moved }, "the room it had");
        // The next one in what room is left, beside the last.
        let c = ws.split_pane(&a, SplitDir::Row).unwrap();
        assert_eq!(ws.tabs[0].cards[&c], BoardRect { x: inner.0 + moved.w + board::CARD_GAP, y: inner.1, ..moved });
        assert_nothing_overlaps(&ws);
        // And moving a card changes no split.
        let root = ws.tabs[0].root.clone();
        ws.set_card_rect(&b, BoardRect { x: 16.0, y: 36.0, w: 200.0, h: 200.0 });
        assert_eq!(ws.tabs[0].root, root);
        assert_eq!(ws.set_card_rect("gone", moved), None);
    }

    #[test]
    fn the_last_tab_closed_comes_back_where_its_frame_was() {
        let mut ws = Workspace::default();
        let id = ws.tabs[0].id.clone();
        ws.set_frame_position(&id, BoardPoint { x: 700.0, y: -40.0 });
        ws.close_tab(&id);
        assert_eq!(ws.tabs[0].frame, BoardPoint { x: 700.0, y: -40.0 });
        assert_board_consistent(&ws);
    }

    #[test]
    fn the_snapshot_carries_the_board() {
        let mut ws = Workspace::default();
        let a = ws.focused_pane();
        let b = ws.split_pane(&a, SplitDir::Column).unwrap();
        assert!(ws.set_view(View::Board));
        assert!(!ws.set_view(View::Board), "already the board");
        ws.camera = Camera { x: -20.0, y: 10.0, zoom: 0.5 };
        let snap = ws.snapshot(&HashMap::new());
        assert_eq!(snap.view, View::Board);
        assert_eq!(snap.camera, ws.camera);
        let tab = &snap.tabs[0];
        let ids: Vec<&str> = tab.cards.iter().map(|c| c.pane_id.as_str()).collect();
        assert_eq!(ids, vec![a.as_str(), b.as_str()], "in the tab's pane order");
        assert_eq!(tab.frame, ws.tabs[0].frame_rect());
    }

    #[test]
    fn a_card_or_frame_dropped_on_a_taken_place_stays_put() {
        let mut ws = Workspace::default();
        let a = ws.focused_pane();
        let b = ws.split_pane(&a, SplitDir::Row).unwrap();
        ws.new_tab();
        let was = ws.tabs[0].cards[&b];
        // Onto its neighbour: refused, and the answer says where it is.
        let onto_a = BoardRect { x: 100.0, ..was };
        assert_eq!(ws.set_card_rect(&b, onto_a), Some(was));
        // Free, below: moved.
        let below = BoardRect { y: 1000.0, ..was };
        assert_eq!(ws.set_card_rect(&b, below), Some(below));
        // A frame onto another frame: refused; clear of it: moved.
        let first = ws.tabs[0].id.clone();
        let at = ws.tabs[0].frame;
        assert_eq!(ws.set_frame_position(&first, BoardPoint { x: 2000.0, y: 100.0 }), Some(at));
        assert_eq!(ws.set_frame_position(&first, BoardPoint { x: 0.0, y: 3000.0 }), Some(BoardPoint { x: 0.0, y: 3000.0 }));
        assert_eq!(ws.set_frame_position("gone", at), None);
    }

    #[test]
    fn arranging_moves_frames_with_their_cards() {
        let mut ws = Workspace::default();
        let a = ws.focused_pane();
        ws.split_pane(&a, SplitDir::Row).unwrap();
        ws.new_tab();
        ws.new_tab();
        let ids: Vec<String> = ws.tabs.iter().map(|t| t.id.clone()).collect();
        // Strewn about, one far off. The first moves last, once the others
        // are out of its way.
        ws.set_frame_position(&ids[1], BoardPoint { x: 9000.0, y: 7000.0 });
        ws.set_frame_position(&ids[2], BoardPoint { x: 4000.0, y: -400.0 });
        ws.set_frame_position(&ids[0], BoardPoint { x: 300.0, y: 200.0 });
        assert_eq!(ws.tabs[0].frame, BoardPoint { x: 300.0, y: 200.0 });
        let cards = ws.tabs[0].cards.clone();
        ws.arrange_frames(1600.0, 1000.0);
        assert_eq!(ws.tabs[0].frame, BoardPoint { x: 300.0, y: -400.0 }, "from the frames' top-left");
        assert_eq!(ws.tabs[0].cards, cards, "cards stay put in their frame");
        assert_nothing_overlaps(&ws);
        let far = ws.tabs.iter().map(|t| t.frame.x.max(t.frame.y)).fold(f64::MIN, f64::max);
        assert!(far < 4000.0, "packed: {far}");
    }

    /// No two frames overlap, and no two cards anywhere on the board.
    fn assert_nothing_overlaps(ws: &Workspace) {
        let apart = |a: &BoardRect, b: &BoardRect| {
            a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y
        };
        let frames: Vec<BoardRect> = ws.tabs.iter().map(Tab::frame_rect).collect();
        let cards: Vec<BoardRect> = ws
            .tabs
            .iter()
            .flat_map(|t| t.cards.values().map(move |c| BoardRect { x: t.frame.x + c.x, y: t.frame.y + c.y, ..*c }))
            .collect();
        for rects in [frames, cards] {
            for (i, a) in rects.iter().enumerate() {
                for b in &rects[i + 1..] {
                    assert!(apart(a, b), "{a:?} overlaps {b:?}");
                }
            }
        }
    }

    #[test]
    fn a_split_growing_its_frame_pushes_the_next_frame_clear() {
        let mut ws = Workspace::default();
        let a = ws.focused_pane();
        let (_, other) = ws.new_tab();
        // The split's card goes right of `a`, where the second frame was:
        // that frame moves on, and its card can't be stacked on.
        ws.split_pane(&a, SplitDir::Row);
        assert_nothing_overlaps(&ws);
        // Neither by a card of the first tab moved or resized onto it.
        let theirs = ws.tabs[1].frame.x + ws.tabs[1].cards[&other].x;
        let onto = BoardRect { x: theirs, y: 42.0, w: 400.0, h: 300.0 };
        ws.set_card_rect(&a, onto);
        assert_nothing_overlaps(&ws);
    }

    /// A tiny deterministic generator: no `rand` dependency for one test.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self, n: usize) -> usize {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 33) as usize) % n
        }
    }

    #[test]
    fn nothing_on_the_board_ever_overlaps() {
        let mut rng = Lcg(7);
        let mut ws = Workspace::default();
        for _ in 0..400 {
            let panes = ws.all_pane_ids();
            let pane = panes[rng.next(panes.len())].clone();
            let tab = ws.tabs[rng.next(ws.tabs.len())].id.clone();
            let n = |rng: &mut Lcg, max: usize| rng.next(max) as f64;
            match rng.next(7) {
                0 => {
                    ws.new_tab();
                }
                1 => {
                    ws.split_pane(&pane, SplitDir::Row);
                }
                2 => {
                    ws.split_pane(&pane, SplitDir::Column);
                }
                3 => {
                    ws.close_pane(&pane);
                }
                4 => {
                    let rect = board::card(n(&mut rng, 3000), n(&mut rng, 2000), 160.0 + n(&mut rng, 1500), 100.0 + n(&mut rng, 1000)).unwrap();
                    ws.set_card_rect(&pane, rect);
                }
                5 => {
                    ws.set_frame_position(&tab, BoardPoint { x: n(&mut rng, 20000), y: n(&mut rng, 8000) });
                }
                _ => {
                    ws.adopt_pane(&format!("held-{}", rng.next(1_000_000)));
                }
            }
            assert_nothing_overlaps(&ws);
        }
    }

    #[test]
    fn a_card_says_what_its_pane_is() {
        assert_eq!(pane_title(None), "shell");
        let meta = |m: PaneMeta| pane_title(Some(&m));
        assert_eq!(meta(PaneMeta { cwd: Some("/tmp/project".into()), ..Default::default() }), "project");
        assert_eq!(
            meta(PaneMeta { cwd: Some("/tmp/project".into()), title: Some("vim: main.rs".into()), ..Default::default() }),
            "vim: main.rs"
        );
        assert_eq!(meta(PaneMeta { title: Some(r"C:\pwsh.exe".into()), ..Default::default() }), "shell");
        assert_eq!(meta(PaneMeta { diff_repo: Some("/src/mirador".into()), ..Default::default() }), "Diff · mirador");
        assert_eq!(meta(PaneMeta { browser_url: Some("https://example.com/".into()), ..Default::default() }), "https://example.com/");
        assert_eq!(meta(PaneMeta { agent_wall: true, title: Some("x".into()), ..Default::default() }), "Agents");
        assert_eq!(meta(PaneMeta { task_board: true, ..Default::default() }), "Tasks");
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
