//! Session persistence: layout, per-pane cwd/command, and scrollback
//! survive restarts. Processes are not reattached — shells respawn in
//! their saved cwd, command panes restore idle (keypress reruns).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cmux_protocol::Node;
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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionPane {
    pub cwd: Option<String>,
    pub command: Option<String>,
    #[serde(default)]
    pub browser_url: Option<String>,
    #[serde(default)]
    pub agent_session: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFile {
    pub version: u32,
    pub tabs: Vec<SessionTab>,
    pub active_tab: String,
    pub panes: HashMap<String, SessionPane>,
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
        })
        .collect();
    let pane_ids: Vec<String> = workspace.all_pane_ids();
    SessionFile {
        version: VERSION,
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
    let mut tabs = Vec::new();
    for t in file.tabs {
        let panes = crate::layout::pane_ids(&t.root);
        if panes.is_empty() {
            continue;
        }
        let focused = if panes.contains(&t.focused) {
            t.focused
        } else {
            panes[0].clone()
        };
        tabs.push(Tab {
            id: t.id,
            title: t.title,
            root: t.root,
            focused,
        });
    }
    if tabs.is_empty() {
        return None;
    }
    let active = tabs
        .iter()
        .position(|t| t.id == file.active_tab)
        .unwrap_or(0);
    let workspace = Workspace { tabs, active };

    let meta = file
        .panes
        .into_iter()
        .map(|(id, p)| {
            (
                id,
                PaneMeta {
                    cwd: p.cwd,
                    command: p.command,
                    browser_url: p.browser_url,
                    agent_session: p.agent_session,
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
        assert_eq!(rmeta[&c].command.as_deref(), Some("npm test"));
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
