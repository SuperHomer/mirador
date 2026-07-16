//! Listening-port detection for the sidebar: which TCP ports do the
//! processes inside each pane hold open?
//!
//! One `ps` + one `lsof` per poll covers every pane at once. lsof ships
//! with macOS and is common on Linux; absence degrades to empty results.
//! (A /proc- and GetExtendedTcpTable-based backend lands with M11.)

use std::collections::{HashMap, HashSet};
use std::process::Command;

/// pid → listening TCP ports, for all processes of the current user.
pub fn listening_ports_by_pid() -> HashMap<u32, Vec<u16>> {
    let output = Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpn"])
        .output();
    let Ok(output) = output else {
        return HashMap::new();
    };
    let mut map: HashMap<u32, Vec<u16>> = HashMap::new();
    let mut current_pid: Option<u32> = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(pid) = line.strip_prefix('p') {
            current_pid = pid.parse().ok();
        } else if let (Some(pid), Some(name)) = (current_pid, line.strip_prefix('n')) {
            // e.g. "*:3000", "127.0.0.1:8080", "[::1]:5173"
            if let Some(port) = name.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()) {
                let ports = map.entry(pid).or_default();
                if !ports.contains(&port) {
                    ports.push(port);
                }
            }
        }
    }
    map
}

/// parent pid → child pids, for all processes.
pub fn process_children() -> HashMap<u32, Vec<u32>> {
    let output = Command::new("ps").args(["-axo", "pid=,ppid="]).output();
    let Ok(output) = output else {
        return HashMap::new();
    };
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut parts = line.split_whitespace();
        if let (Some(pid), Some(ppid)) = (
            parts.next().and_then(|s| s.parse().ok()),
            parts.next().and_then(|s| s.parse().ok()),
        ) {
            children.entry(ppid).or_default().push(pid);
        }
    }
    children
}

/// All descendants of `root` (inclusive).
pub fn process_tree(root: u32, children: &HashMap<u32, Vec<u32>>) -> HashSet<u32> {
    let mut seen = HashSet::new();
    let mut queue = vec![root];
    while let Some(pid) = queue.pop() {
        if seen.insert(pid) {
            if let Some(kids) = children.get(&pid) {
                queue.extend(kids);
            }
        }
    }
    seen
}

/// Listening ports owned by each pane's process tree.
pub fn ports_for_panes(pane_pids: &[(String, u32)]) -> HashMap<String, Vec<u16>> {
    if pane_pids.is_empty() {
        return HashMap::new();
    }
    let by_pid = listening_ports_by_pid();
    if by_pid.is_empty() {
        return HashMap::new();
    }
    let children = process_children();
    let mut result = HashMap::new();
    for (pane, root_pid) in pane_pids {
        let tree = process_tree(*root_pid, &children);
        let mut ports: Vec<u16> = by_pid
            .iter()
            .filter(|(pid, _)| tree.contains(pid))
            .flat_map(|(_, ports)| ports.iter().copied())
            .collect();
        ports.sort_unstable();
        ports.dedup();
        if !ports.is_empty() {
            result.insert(pane.clone(), ports);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn own_listener_is_detected() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let me = std::process::id();

        let ports = ports_for_panes(&[("pane".to_string(), me)]);
        let mine = ports.get("pane").cloned().unwrap_or_default();
        assert!(
            mine.contains(&port),
            "own listener on {port} not found in {mine:?}"
        );
        drop(listener);
    }

    #[test]
    fn process_tree_includes_root() {
        let children = HashMap::new();
        let tree = process_tree(42, &children);
        assert!(tree.contains(&42));
    }
}
