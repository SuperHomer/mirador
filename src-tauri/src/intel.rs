//! Sidebar intelligence poller: per-pane cwd + git branch every tick (2s),
//! listening ports every other tick, PR status via `gh` every 30 ticks
//! (60s) or when a new (repo, branch) appears. Emits a fresh snapshot only
//! when something actually changed.

use std::process::Command;
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

            // cwd + branch every tick.
            for (pane, pid) in &pids {
                let cwd = cmux_core::cwd::process_cwd(*pid);
                let git = cwd.as_deref().and_then(cmux_core::git::branch_for_cwd);
                let mut meta = state.meta.lock().unwrap();
                let entry = meta.entry(pane.clone()).or_default();
                if let Some(cwd) = cwd {
                    if entry.cwd.as_deref() != Some(cwd.as_str()) {
                        entry.cwd = Some(cwd);
                        changed = true;
                    }
                }
                let (root, branch) = match git {
                    Some((root, branch)) => {
                        (Some(root.to_string_lossy().to_string()), Some(branch))
                    }
                    None => (None, None),
                };
                if entry.branch != branch || entry.repo_root != root {
                    entry.branch = branch;
                    entry.repo_root = root;
                    changed = true;
                }
            }

            // Ports every other tick (~4s).
            if tick.is_multiple_of(2) {
                let ports = cmux_core::ports::ports_for_panes(&pids);
                let mut meta = state.meta.lock().unwrap();
                for (pane, _) in &pids {
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
                let refresh_all = tick.is_multiple_of(30);
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
    Command::new("gh")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `gh pr view <branch>` in the repo root. None = no PR / error — both fine.
fn fetch_pr(repo_root: &str, branch: &str) -> Option<PrStatus> {
    let output = Command::new("gh")
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
