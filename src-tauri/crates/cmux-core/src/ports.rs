//! Listening-port detection for the sidebar: which TCP ports do the
//! processes inside each pane hold open?
//!
//! On macOS/Linux one `ps` + one `lsof` per poll covers every pane at once;
//! lsof ships with macOS and is common on Linux, and its absence degrades
//! to empty results. Windows has no such tool, so it asks the OS directly
//! (GetExtendedTcpTable + a Toolhelp process snapshot).

use std::collections::{HashMap, HashSet};

#[cfg(not(windows))]
mod backend {
    use super::HashMap;
    use crate::proc::command;

    /// pid → listening TCP ports, for all processes of the current user.
    pub fn listening_ports_by_pid() -> HashMap<u32, Vec<u16>> {
        let output = command("lsof")
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
        let output = command("ps").args(["-axo", "pid=,ppid="]).output();
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
}

#[cfg(windows)]
mod backend {
    use super::HashMap;
    use std::os::raw::c_void;

    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6TABLE_OWNER_PID, MIB_TCPTABLE_OWNER_PID,
        TCP_TABLE_OWNER_PID_LISTENER,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    /// The table APIs want a buffer sized by a first, failing call; the
    /// table can grow between the two, hence the retries.
    fn tcp_table(family: u32) -> Vec<u8> {
        let mut size: u32 = 0;
        for _ in 0..4 {
            let result = unsafe {
                GetExtendedTcpTable(
                    std::ptr::null_mut(),
                    &mut size,
                    0,
                    family,
                    TCP_TABLE_OWNER_PID_LISTENER,
                    0,
                )
            };
            if result != NO_ERROR && result != ERROR_INSUFFICIENT_BUFFER {
                return Vec::new();
            }
            let mut buf = vec![0u8; size as usize];
            let result = unsafe {
                GetExtendedTcpTable(
                    buf.as_mut_ptr() as *mut c_void,
                    &mut size,
                    0,
                    family,
                    TCP_TABLE_OWNER_PID_LISTENER,
                    0,
                )
            };
            if result == NO_ERROR {
                return buf;
            }
            if result != ERROR_INSUFFICIENT_BUFFER {
                return Vec::new();
            }
        }
        Vec::new()
    }

    /// dwLocalPort holds the port in network byte order in its low word.
    fn port_of(raw: u32) -> u16 {
        u16::from_be(raw as u16)
    }

    fn add(map: &mut HashMap<u32, Vec<u16>>, pid: u32, port: u16) {
        let ports = map.entry(pid).or_default();
        if !ports.contains(&port) {
            ports.push(port);
        }
    }

    pub fn listening_ports_by_pid() -> HashMap<u32, Vec<u16>> {
        let mut map: HashMap<u32, Vec<u16>> = HashMap::new();

        let v4 = tcp_table(AF_INET as u32);
        if v4.len() >= std::mem::size_of::<u32>() {
            let table = v4.as_ptr() as *const MIB_TCPTABLE_OWNER_PID;
            let count = unsafe { (*table).dwNumEntries } as usize;
            for i in 0..count {
                let row = unsafe { &*(*table).table.as_ptr().add(i) };
                add(&mut map, row.dwOwningPid, port_of(row.dwLocalPort));
            }
        }

        let v6 = tcp_table(AF_INET6 as u32);
        if v6.len() >= std::mem::size_of::<u32>() {
            let table = v6.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID;
            let count = unsafe { (*table).dwNumEntries } as usize;
            for i in 0..count {
                let row = unsafe { &*(*table).table.as_ptr().add(i) };
                add(&mut map, row.dwOwningPid, port_of(row.dwLocalPort));
            }
        }

        map
    }

    /// Windows does not reparent orphans, so an entry can name a parent pid
    /// that has since been recycled. The worst case is a stray port chip on
    /// a pane; the snapshot is retaken every poll.
    pub fn process_children() -> HashMap<u32, Vec<u32>> {
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot.is_null() {
            return children;
        }
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) };
        while ok != 0 {
            children
                .entry(entry.th32ParentProcessID)
                .or_default()
                .push(entry.th32ProcessID);
            ok = unsafe { Process32NextW(snapshot, &mut entry) };
        }
        unsafe { CloseHandle(snapshot) };
        children
    }
}

pub use backend::{listening_ports_by_pid, process_children};

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
