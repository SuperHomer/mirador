//! Session persistence: layout, per-pane cwd/command, and scrollback
//! survive restarts. Processes are not reattached — shells respawn in
//! their saved cwd, command panes restore idle (keypress reruns).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cmux_protocol::{BoardPoint, BoardRect, Camera, Node, View};
use serde::{Deserialize, Serialize};

use crate::state::{PaneMeta, Tab, Workspace};

const VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTab {
    pub id: String,
    /// Explicit user rename only (derived titles are recomputed).
    pub title: Option<String>,
    pub root: Node,
    pub focused: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<cmux_protocol::TeamRun>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wall: bool,
    /// Absent from sessions saved before the board: the tab gets a frame
    /// and cards from its splits when it is restored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<BoardPoint>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub cards: HashMap<String, BoardRect>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionPane {
    pub cwd: Option<String>,
    pub command: Option<String>,
    #[serde(default)]
    pub browser_url: Option<String>,
    #[serde(default)]
    pub agent_session: Option<String>,
    #[serde(default)]
    pub remote_host: Option<String>,
    #[serde(default)]
    pub diff_repo: Option<String>,
    #[serde(default)]
    pub diff_spec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_role: Option<String>,
    /// The settings file the role's Claude Code started with: a resume
    /// passes it again, or the agent would come back without its limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_settings: Option<String>,
    /// The wall was a pane of its own before it was a way of showing a
    /// tab. Read so that such a pane is dropped on restore rather than
    /// coming back as a shell; never written.
    #[serde(default, skip_serializing)]
    pub agent_wall: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub agent_editor: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub task_board: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_board_tab: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFile {
    pub version: u32,
    pub tabs: Vec<SessionTab>,
    pub active_tab: String,
    pub panes: HashMap<String, SessionPane>,
    #[serde(default)]
    pub view: View,
    #[serde(default)]
    pub camera: Camera,
}

pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
            .join("Library/Application Support/Mirador")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
                    .join(".local/share")
            })
            .join("mirador")
    }
    #[cfg(windows)]
    {
        PathBuf::from(std::env::var("APPDATA").unwrap_or_else(|_| ".".into())).join("mirador")
    }
}

pub fn session_path() -> PathBuf {
    data_dir().join("session.json")
}

pub fn scrollback_dir() -> PathBuf {
    data_dir().join("scrollback")
}

pub fn scrollback_path(pane_id: &str) -> PathBuf {
    // Pane ids are uuids we generate; still, never trust them as paths.
    let safe: String = pane_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    scrollback_dir().join(format!("{safe}.scrollback"))
}

/// Crash-safe write: temp file + rename.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

pub fn capture(workspace: &Workspace, meta: &HashMap<String, PaneMeta>) -> SessionFile {
    let tabs: Vec<SessionTab> = workspace
        .tabs
        .iter()
        .map(|t| SessionTab {
            id: t.id.clone(),
            title: t.title.clone(),
            root: t.root.clone(),
            focused: t.focused.clone(),
            team: t.team.clone(),
            wall: t.wall,
            frame: Some(t.frame),
            cards: t.cards.clone(),
        })
        .collect();
    let pane_ids: Vec<String> = workspace.all_pane_ids();
    SessionFile {
        version: VERSION,
        view: workspace.view,
        camera: workspace.camera,
        active_tab: workspace.tabs[workspace.active].id.clone(),
        panes: pane_ids
            .into_iter()
            .map(|id| {
                let m = meta.get(&id);
                (
                    id,
                    SessionPane {
                        cwd: m.and_then(|m| m.cwd.clone()),
                        command: m.and_then(|m| m.command.clone()),
                        browser_url: m.and_then(|m| m.browser_url.clone()),
                        agent_session: m.and_then(|m| m.agent_session.clone()),
                        remote_host: m.and_then(|m| m.remote_host.clone()),
                        diff_repo: m.and_then(|m| m.diff_repo.clone()),
                        diff_spec: m.and_then(|m| m.diff_spec.clone()),
                        agent_role: m.and_then(|m| m.agent_role.clone()),
                        agent_settings: m.and_then(|m| m.agent_settings.clone()),
                        agent_wall: false,
                        agent_editor: m.is_some_and(|m| m.agent_editor),
                        task_board: m.is_some_and(|m| m.task_board),
                        task_board_tab: m.and_then(|m| m.task_board_tab.clone()),
                    },
                )
            })
            .collect(),
        tabs,
    }
}

pub fn save(file: &SessionFile) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(file)?;
    atomic_write(&session_path(), &json)
}

pub fn load() -> Option<SessionFile> {
    let text = std::fs::read_to_string(session_path()).ok()?;
    let file: SessionFile = serde_json::from_str(&text).ok()?;
    if file.version != VERSION || file.tabs.is_empty() {
        return None;
    }
    Some(file)
}

/// Rebuilds workspace + meta from a session. Invalid entries are dropped;
/// an unusable session yields None (caller starts fresh).
pub fn restore(file: SessionFile) -> Option<(Workspace, HashMap<String, PaneMeta>)> {
    let old_walls: Vec<&String> = file
        .panes
        .iter()
        .filter(|(_, p)| p.agent_wall)
        .map(|(id, _)| id)
        .collect();
    let mut tabs = Vec::new();
    let mut unplaced = Vec::new();
    for mut t in file.tabs {
        let emptied = old_walls.iter().any(|w| {
            matches!(crate::layout::remove(&mut t.root, w), crate::layout::RemoveOutcome::BecameEmpty)
        });
        let panes = crate::layout::pane_ids(&t.root);
        if emptied || panes.is_empty() {
            continue;
        }
        let focused = if panes.contains(&t.focused) {
            t.focused
        } else {
            panes[0].clone()
        };
        let mut cards = t.cards;
        crate::board::settle(&t.root, &mut cards);
        if t.frame.is_none() {
            unplaced.push(t.id.clone());
        }
        tabs.push(Tab {
            id: t.id,
            title: t.title,
            root: t.root,
            focused,
            zoomed: None,
            team: t.team,
            wall: t.wall,
            frame: t.frame.unwrap_or_default(),
            cards,
        });
    }
    if tabs.is_empty() {
        return None;
    }
    let active = tabs
        .iter()
        .position(|t| t.id == file.active_tab)
        .unwrap_or(0);
    // Frames the session did not place go in a row, in tab order, right
    // of every frame it did.
    let mut placed: Vec<BoardRect> = tabs
        .iter()
        .filter(|t| !unplaced.contains(&t.id))
        .map(Tab::frame_rect)
        .collect();
    for tab in tabs.iter_mut().filter(|t| unplaced.contains(&t.id)) {
        tab.frame = crate::board::next_frame_origin(placed.iter().copied());
        placed.push(tab.frame_rect());
    }
    let mut workspace = Workspace {
        tabs,
        active,
        view: file.view,
        camera: file.camera,
        viewport: None,
    };
    // A session written before frames were kept apart may have them
    // overlapping; from here on they never do.
    let ids: Vec<String> = workspace.tabs.iter().map(|t| t.id.clone()).collect();
    for id in ids {
        workspace.push_frames_clear(&id);
    }

    let meta = file
        .panes
        .into_iter()
        .filter(|(_, p)| !p.agent_wall)
        .map(|(id, p)| {
            (
                id,
                PaneMeta {
                    cwd: p.cwd,
                    command: p.command,
                    browser_url: p.browser_url,
                    agent_session: p.agent_session,
                    remote_host: p.remote_host,
                    diff_repo: p.diff_repo,
                    diff_spec: p.diff_spec,
                    agent_role: p.agent_role,
                    agent_settings: p.agent_settings,
                    agent_editor: p.agent_editor,
                    task_board: p.task_board,
                    task_board_tab: p.task_board_tab,
                    ..Default::default()
                },
            )
        })
        .collect();
    Some((workspace, meta))
}

pub fn save_scrollback(pane_id: &str, data: &str) -> std::io::Result<()> {
    atomic_write(&scrollback_path(pane_id), data.as_bytes())
}

pub fn load_scrollback(pane_id: &str) -> Option<String> {
    std::fs::read_to_string(scrollback_path(pane_id)).ok()
}

pub fn delete_scrollback(pane_id: &str) {
    let _ = std::fs::remove_file(scrollback_path(pane_id));
}

/// Drops scrollback files for panes no longer in the session.
pub fn gc_scrollback(live_panes: &[String]) {
    let Ok(entries) = std::fs::read_dir(scrollback_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(pane) = name.strip_suffix(".scrollback") else {
            continue;
        };
        if !live_panes.iter().any(|p| p == pane) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmux_protocol::SplitDir;

    #[test]
    fn a_team_comes_back_with_its_tab() {
        let mut ws = Workspace::default();
        let panes = Workspace::fresh_pane_ids(2);
        let run = cmux_protocol::TeamRun {
            name: "squad".into(),
            members: vec![
                cmux_protocol::TeamSeat { name: "lead".into(), role: "planner".into(), pane: panes[0].clone(), lead: true },
                cmux_protocol::TeamSeat { name: "dev".into(), role: "dev".into(), pane: panes[1].clone(), lead: false },
            ],
        };
        let tab = ws.new_team_tab(crate::teams::layout(&panes, None), run.clone(), "squad");
        let file = capture(&ws, &HashMap::new());
        let json = serde_json::to_string(&file).unwrap();
        let (back, _) = restore(serde_json::from_str(&json).unwrap()).unwrap();
        let t = back.tabs.iter().find(|t| t.id == tab).unwrap();
        assert_eq!(t.team.as_ref(), Some(&run));
        assert_eq!(t.title.as_deref(), Some("squad"));
        // A tab with no team writes nothing for it.
        assert!(!json.contains(r#""team":null"#));
    }

    #[test]
    fn a_wall_comes_back_with_its_tab() {
        let mut ws = Workspace::default();
        let walled = ws.focused_pane();
        let (_, plain) = ws.new_tab();
        ws.toggle_wall(&walled);
        let json = serde_json::to_string(&capture(&ws, &HashMap::new())).unwrap();
        let (back, _) = restore(serde_json::from_str(&json).unwrap()).unwrap();
        assert!(back.tab_of_pane(&walled).unwrap().wall);
        assert!(!back.tab_of_pane(&plain).unwrap().wall);
        // Only the wall's tab writes the key.
        assert_eq!(json.matches(r#""wall":"#).count(), 1);
    }

    #[test]
    fn capture_restore_roundtrip() {
        let mut ws = Workspace::default();
        let a = ws.focused_pane();
        let b = ws.split_pane(&a, SplitDir::Row).unwrap();
        ws.rename_tab(&ws.tabs[0].id.clone(), "my tab");
        let (_tab2, c) = ws.new_tab();

        let mut meta = HashMap::new();
        meta.insert(
            a.clone(),
            PaneMeta {
                cwd: Some("/tmp".into()),
                ..Default::default()
            },
        );
        meta.insert(
            c.clone(),
            PaneMeta {
                command: Some("npm test".into()),
                task_board: true,
                task_board_tab: Some("tab-1".into()),
                ..Default::default()
            },
        );
        meta.insert(
            b.clone(),
            PaneMeta {
                agent_session: Some("claude-fg:s1".into()),
                agent_role: Some("reviewer".into()),
                agent_settings: Some("/data/prompts/reviewer-1.settings.json".into()),
                agent_status: Some(cmux_protocol::AgentStatus::Working),
                ..Default::default()
            },
        );

        let file = capture(&ws, &meta);
        let json = serde_json::to_string(&file).unwrap();
        let parsed: SessionFile = serde_json::from_str(&json).unwrap();
        let (restored, rmeta) = restore(parsed).unwrap();

        assert_eq!(restored.tabs.len(), 2);
        assert_eq!(restored.tabs[0].title.as_deref(), Some("my tab"));
        assert_eq!(restored.active, 1);
        assert!(crate::layout::contains(&restored.tabs[0].root, &a));
        assert!(crate::layout::contains(&restored.tabs[0].root, &b));
        assert_eq!(rmeta[&a].cwd.as_deref(), Some("/tmp"));
        // The role survives so a resume gets its model; the status is only
        // ever what this run heard.
        assert_eq!(rmeta[&b].agent_role.as_deref(), Some("reviewer"));
        assert_eq!(rmeta[&b].agent_settings.as_deref(), Some("/data/prompts/reviewer-1.settings.json"));
        assert_eq!(rmeta[&b].agent_status, None);
        assert_eq!(rmeta[&c].command.as_deref(), Some("npm test"));
        assert!(rmeta[&c].task_board && !rmeta[&a].task_board);
        assert_eq!(rmeta[&c].task_board_tab.as_deref(), Some("tab-1"));
    }

    #[test]
    fn the_board_comes_back_as_it_was_left() {
        let mut ws = Workspace::default();
        let a = ws.focused_pane();
        let (tab2, c) = ws.new_tab();
        ws.set_view(View::Board);
        ws.camera = Camera { x: 120.0, y: -60.0, zoom: 0.75 };
        let card = BoardRect { x: 50.0, y: 90.0, w: 640.0, h: 400.0 };
        ws.set_card_rect(&c, card);
        ws.set_frame_position(&tab2, BoardPoint { x: 3000.0, y: 500.0 });
        let json = serde_json::to_string(&capture(&ws, &HashMap::new())).unwrap();
        let (back, _) = restore(serde_json::from_str(&json).unwrap()).unwrap();
        assert_eq!(back.view, View::Board);
        assert_eq!(back.camera, ws.camera);
        assert_eq!(back.tabs[1].frame, BoardPoint { x: 3000.0, y: 500.0 });
        assert_eq!(back.tabs[1].cards[&c], card);
        assert_eq!(back.tabs[0].cards[&a], ws.tabs[0].cards[&a]);
    }

    #[test]
    fn cards_saved_overlapping_come_back_apart() {
        let mut ws = Workspace::default();
        let a = ws.focused_pane();
        let b = ws.split_pane(&a, SplitDir::Row).unwrap();
        let mut file = capture(&ws, &HashMap::new());
        // `b` saved on top of `a`, as an older or hand-edited file can.
        let first = file.tabs[0].cards[&a];
        file.tabs[0].cards.insert(b.clone(), BoardRect { x: first.x + 50.0, y: first.y + 50.0, ..first });
        let (back, _) = restore(file).unwrap();
        let cards = &back.tabs[0].cards;
        assert_eq!(cards[&a], first, "the first card where it was");
        let (x, y) = (cards[&a], cards[&b]);
        assert!(x.x + x.w <= y.x || y.x + y.w <= x.x || x.y + x.h <= y.y || y.y + y.h <= x.y, "{x:?} {y:?}");
    }

    #[test]
    fn frames_saved_overlapping_come_back_apart() {
        let mut ws = Workspace::default();
        ws.new_tab();
        let mut file = capture(&ws, &HashMap::new());
        file.tabs[1].frame = Some(BoardPoint { x: 50.0, y: 50.0 });
        let (back, _) = restore(file).unwrap();
        let (a, b) = (back.tabs[0].frame_rect(), back.tabs[1].frame_rect());
        assert!(b.x >= a.x + a.w, "{a:?} {b:?}");
    }

    #[test]
    fn a_session_saved_before_the_board_gets_one() {
        // What a release before the board wrote: no view, camera, frames
        // or cards.
        let json = r#"{
            "version": 1,
            "tabs": [
                {"id": "t1", "title": null, "focused": "a",
                 "root": {"type": "split", "dir": "row", "ratios": [0.5, 0.5],
                          "children": [{"type": "leaf", "paneId": "a"}, {"type": "leaf", "paneId": "b"}]}},
                {"id": "t2", "title": "two", "focused": "c",
                 "root": {"type": "leaf", "paneId": "c"}}
            ],
            "active_tab": "t2",
            "panes": {"a": {"cwd": "/tmp"}, "b": {}, "c": {}}
        }"#;
        let (ws, _) = restore(serde_json::from_str(json).unwrap()).unwrap();
        assert_eq!(ws.view, View::Tabs);
        assert_eq!(ws.camera, Camera::default());
        // Its splits as cards, its frames in a row in tab order.
        assert_eq!(ws.tabs[0].cards, crate::board::default_cards(&ws.tabs[0].root));
        assert_eq!(ws.tabs[0].frame, BoardPoint::default());
        let first = ws.tabs[0].frame_rect();
        assert_eq!(ws.tabs[1].frame.x, first.x + first.w + crate::board::FRAME_GAP);
        assert!(ws.tabs[1].cards.contains_key("c"));
        // And no tab is a wall.
        assert!(ws.tabs.iter().all(|t| !t.wall));
    }

    #[test]
    fn an_old_wall_pane_is_dropped_on_restore() {
        // The wall was a pane: here alone in a tab of its own, and split
        // beside a shell in another.
        let json = r#"{
            "version": 1,
            "tabs": [
                {"id": "work", "title": null, "focused": "a",
                 "root": {"type": "split", "dir": "row", "ratios": [0.5, 0.5],
                          "children": [{"type": "leaf", "paneId": "a"}, {"type": "leaf", "paneId": "b"}]}},
                {"id": "walltab", "title": null, "focused": "w",
                 "root": {"type": "leaf", "paneId": "w"}},
                {"id": "mixed", "title": null, "focused": "v",
                 "root": {"type": "split", "dir": "row", "ratios": [0.5, 0.5],
                          "children": [{"type": "leaf", "paneId": "c"}, {"type": "leaf", "paneId": "v"}]}}
            ],
            "active_tab": "walltab",
            "panes": {"a": {"cwd": "/tmp"}, "b": {}, "c": {},
                      "w": {"agent_wall": true, "agent_wall_tab": "work"},
                      "v": {"agent_wall": true, "agent_wall_tab": "work"}}
        }"#;
        let (ws, meta) = restore(serde_json::from_str(json).unwrap()).unwrap();
        let ids: Vec<&str> = ws.tabs.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["work", "mixed"], "the wall's own tab goes with it");
        // It was the active tab: the first takes its place.
        assert_eq!(ws.tabs[ws.active].id, "work");
        assert_eq!(crate::layout::pane_ids(&ws.tabs[0].root), ["a", "b"]);
        assert_eq!(crate::layout::pane_ids(&ws.tabs[1].root), ["c"]);
        assert_eq!(ws.tabs[1].focused, "c");
        assert!(!ws.tabs[1].cards.contains_key("v"));
        assert!(!meta.contains_key("w") && !meta.contains_key("v"));
        assert_eq!(meta["a"].cwd.as_deref(), Some("/tmp"));
        // And it is never written again.
        let json = serde_json::to_string(&capture(&ws, &meta)).unwrap();
        assert!(!json.contains("agent_wall"));
    }

    #[test]
    fn a_session_of_only_an_old_wall_restores_nothing() {
        let json = r#"{
            "version": 1,
            "tabs": [{"id": "walltab", "title": null, "focused": "w",
                      "root": {"type": "leaf", "paneId": "w"}}],
            "active_tab": "walltab",
            "panes": {"w": {"agent_wall": true}}
        }"#;
        assert!(restore(serde_json::from_str(json).unwrap()).is_none());
    }

    #[test]
    fn restore_fixes_bad_focus_and_drops_empty() {
        let ws = Workspace::default();
        let meta = HashMap::new();
        let mut file = capture(&ws, &meta);
        file.tabs[0].focused = "nonexistent".into();
        let (restored, _) = restore(file).unwrap();
        assert!(crate::layout::contains(
            &restored.tabs[0].root,
            &restored.tabs[0].focused
        ));
    }

    #[test]
    fn wrong_version_rejected_by_load_shape() {
        let ws = Workspace::default();
        let mut file = capture(&ws, &HashMap::new());
        file.version = 99;
        let json = serde_json::to_string(&file).unwrap();
        let parsed: SessionFile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.version, 99); // load() checks this and returns None
    }
}
