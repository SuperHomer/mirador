//! Sidebar intelligence poller: per-pane cwd + git branch every tick (2s),
//! listening ports every other tick, PR status via `gh` every 30 ticks
//! (60s) or when a new (repo, branch) appears. Emits a fresh snapshot only
//! when something actually changed.

use std::time::Duration;

use cmux_protocol::PrStatus;
use tauri::Manager;

use crate::AppState;

pub fn spawn(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let gh_available = which_gh();
        let mut tick: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_secs(2));
            tick += 1;
            let state = handle.state::<AppState>();
            let mut changed = false;

            let pids = state.pty.pids();

            // Remote panes hold an ssh client locally: its cwd/branch/ports
            // describe this machine, not the remote — skip them entirely.
            let remote_panes: Vec<String> = {
                let meta = state.meta.lock().unwrap();
                meta.iter()
                    .filter(|(_, m)| m.remote_host.is_some())
                    .map(|(id, _)| id.clone())
                    .collect()
            };
            let pids: Vec<(String, u32)> = pids
                .into_iter()
                .filter(|(pane, _)| !remote_panes.contains(pane))
                .collect();

            // cwd + branch every tick.
            for (pane, pid) in &pids {
                // A shell that reports OSC 7 is authoritative. Polling the
                // OS would fight it — on Windows the process working
                // directory never follows PowerShell's `cd`, so the poll
                // would drag the sidebar back to the launch directory
                // between every prompt.
                let (from_shell, known_cwd) = {
                    let meta = state.meta.lock().unwrap();
                    match meta.get(pane) {
                        Some(m) => (m.cwd_from_shell, m.cwd.clone()),
                        None => (false, None),
                    }
                };
                // Syscalls stay outside the lock: this runs every 2s and
                // the UI takes the same mutex.
                let cwd = if from_shell {
                    known_cwd
                } else {
                    cmux_core::cwd::process_cwd(*pid).or(known_cwd)
                };
                let git = cwd
                    .as_deref()
                    .and_then(cmux_core::git::branch_for_cwd)
                    .map(|(root, branch)| (root.to_string_lossy().to_string(), branch));
                // A pane closed while the locks were released must not get
                // its meta back: nothing would ever remove it again, and the
                // PR refresh would go on running `gh` for its branch.
                // Workspace then meta, the order everything else takes them.
                let ws = state.workspace.lock().unwrap();
                let mut meta = state.meta.lock().unwrap();
                if !ws.has_pane(pane) {
                    continue;
                }
                if meta
                    .entry(pane.clone())
                    .or_default()
                    .observe_cwd(cwd, !from_shell, git)
                {
                    changed = true;
                }
            }

            // Agents: any pane whose terminal runs one — Claude Code, Codex,
            // OpenCode, … — counts as an agent pane while it does. Once a
            // pane's agent has been seen running and then is gone, it was
            // exited — forget it, or the next launch would type `claude
            // --resume` into a pane you had moved on from, and the wall
            // would keep a tile for a shell. Finding new agents waits for
            // every other tick; panes that already are agents are checked
            // on each. Windows lists no processes, so there only Claude's
            // hooks make agents, and nothing is ever seen or forgotten.
            let has_agents = {
                let meta = state.meta.lock().unwrap();
                pids.iter()
                    .any(|(pane, _)| meta.get(pane).is_some_and(|m| m.is_agent()))
            };
            if has_agents || tick.is_multiple_of(2) {
                let procs = cmux_core::agents::process_table();
                let running = cmux_core::agents::panes_running_agents(&procs, &pids);
                let mut meta = state.meta.lock().unwrap();
                for (pane, _) in &pids {
                    let Some(entry) = meta.get_mut(pane) else { continue };
                    if entry.observe_agent(running.get(pane), cmux_core::agents::AUTO_RESUME) {
                        changed = true;
                    }
                }
                // Claude Code's own record of each session — its name, and
                // busy or idle — read once per sessions directory, with the
                // lock released: the UI takes it too.
                let wanted: Vec<(String, std::path::PathBuf, Option<u32>, Option<String>)> = pids
                    .iter()
                    .filter_map(|(pane, _)| {
                        let m = meta.get(pane)?;
                        if m.harness() != Some(cmux_protocol::Harness::Claude) {
                            return None;
                        }
                        let dir = cmux_core::agents::sessions_dir(m.agent_transcript.as_deref())?;
                        Some((pane.clone(), dir, m.agent_pid, m.agent_session.clone()))
                    })
                    .collect();
                drop(meta);
                let mut by_dir: std::collections::HashMap<
                    std::path::PathBuf,
                    Vec<cmux_core::agents::ClaudeSession>,
                > = std::collections::HashMap::new();
                for (_, dir, _, _) in &wanted {
                    if !by_dir.contains_key(dir) {
                        by_dir.insert(dir.clone(), cmux_core::agents::claude_sessions(dir));
                    }
                }
                let mut meta = state.meta.lock().unwrap();
                for (pane, dir, pid, session) in &wanted {
                    let found = cmux_core::agents::find_claude_session(
                        &by_dir[dir],
                        *pid,
                        session.as_deref(),
                        running.get(pane).map(|r| r.args.as_str()),
                    );
                    if meta.get_mut(pane).is_some_and(|m| m.apply_claude_session(found)) {
                        changed = true;
                    }
                }
            }

            // Ports every other tick (~4s).
            if tick.is_multiple_of(2) {
                let ports = cmux_core::ports::ports_for_panes(&pids);
                let ws = state.workspace.lock().unwrap();
                let mut meta = state.meta.lock().unwrap();
                for (pane, _) in &pids {
                    // As above: a closed pane stays closed.
                    if !ws.has_pane(pane) {
                        continue;
                    }
                    let entry = meta.entry(pane.clone()).or_default();
                    let new_ports = ports.get(pane).cloned().unwrap_or_default();
                    if entry.ports != new_ports {
                        entry.ports = new_ports;
                        changed = true;
                    }
                }
            }

            // PR status every 30 ticks (60s), or immediately for new branches.
            if gh_available {
                let targets: Vec<(String, String)> = {
                    let meta = state.meta.lock().unwrap();
                    let mut t: Vec<(String, String)> = meta
                        .values()
                        .filter_map(|m| Some((m.repo_root.clone()?, m.branch.clone()?)))
                        .collect();
                    t.sort();
                    t.dedup();
                    t
                };
                // The periodic refresh spawns `gh` per (repo, branch); in the
                // background nobody is looking at the badges, so only new
                // branches are fetched until the window has focus again.
                let focused = state
                    .window_focused
                    .load(std::sync::atomic::Ordering::Relaxed);
                let refresh_all = focused && tick.is_multiple_of(30);
                for (root, branch) in targets {
                    let known = state
                        .pr_cache
                        .lock()
                        .unwrap()
                        .contains_key(&(root.clone(), branch.clone()));
                    if refresh_all || !known {
                        let status = fetch_pr(&root, &branch);
                        let mut cache = state.pr_cache.lock().unwrap();
                        let key = (root.clone(), branch.clone());
                        if cache.get(&key) != Some(&status) {
                            cache.insert(key, status);
                            changed = true;
                        }
                    }
                }
            }

            if changed {
                crate::commands::emit_workspace(&handle);
            }
        }
    });
}

fn which_gh() -> bool {
    cmux_core::proc::command("gh")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `gh pr view <branch>` in the repo root. None = no PR / error — both fine.
fn fetch_pr(repo_root: &str, branch: &str) -> Option<PrStatus> {
    let output = cmux_core::proc::command("gh")
        .args([
            "pr",
            "view",
            branch,
            "--json",
            "number,state,url,statusCheckRollup",
        ])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let rollup = json["statusCheckRollup"].as_array();
    let checks = match rollup {
        None => "none",
        Some(items) if items.is_empty() => "none",
        Some(items) => {
            let conclusions: Vec<&str> = items
                .iter()
                .map(|c| {
                    c["conclusion"]
                        .as_str()
                        .or_else(|| c["state"].as_str())
                        .unwrap_or("")
                })
                .collect();
            if conclusions
                .iter()
                .any(|c| matches!(*c, "FAILURE" | "ERROR" | "CANCELLED" | "TIMED_OUT"))
            {
                "fail"
            } else if conclusions
                .iter()
                .any(|c| matches!(*c, "" | "PENDING" | "IN_PROGRESS" | "QUEUED" | "EXPECTED"))
            {
                "pending"
            } else {
                "pass"
            }
        }
    };
    Some(PrStatus {
        number: json["number"].as_u64()?,
        state: json["state"].as_str().unwrap_or("OPEN").to_string(),
        url: json["url"].as_str().unwrap_or("").to_string(),
        checks: checks.to_string(),
    })
}
