//! A stand-in for `claude` and `codex` in end-to-end tests: linked into the
//! sandbox's PATH under those names, so Mirador's process table sees a
//! `claude` (or `codex`) on the pane's terminal, exactly as for the real
//! one — a shell script would show up as `/bin/sh`.
//!
//! It logs how it was started, one JSON line per event, to `E2E_AGENT_LOG`;
//! as `claude`, it tells Mirador its session through the real `mira
//! claude-hook session-start` (`E2E_MIRA`), as Claude Code's hooks would —
//! resuming the session `--resume` names, or a new one; then it echoes
//! what is typed into it until `/exit`.

use std::io::{BufRead, Write};

fn log(event: serde_json::Value) {
    let Ok(path) = std::env::var("E2E_AGENT_LOG") else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{event}");
    }
}

fn main() {
    let mut args = std::env::args();
    let argv0 = args.next().unwrap_or_default();
    let kind = std::path::Path::new(&argv0)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let args: Vec<String> = args.collect();
    let pid = std::process::id();
    let pane = std::env::var("MIRA_PANE").ok();
    log(serde_json::json!({
        "event": "start",
        "kind": kind,
        "pid": pid,
        "args": args,
        "pane": pane,
        "cwd": std::env::current_dir().ok(),
    }));

    if kind == "claude" {
        let session = args
            .iter()
            .position(|a| a == "--resume")
            .and_then(|i| args.get(i + 1).cloned())
            .unwrap_or_else(|| {
                let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
                format!("fake-{pid}-{}", t.as_millis())
            });
        if let Ok(mira) = std::env::var("E2E_MIRA") {
            let ok = std::process::Command::new(mira)
                .args(["claude-hook", "session-start"])
                .env_remove("CLAUDE_JOB_DIR")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    child
                        .stdin
                        .take()
                        .unwrap()
                        .write_all(serde_json::json!({ "session_id": session }).to_string().as_bytes())?;
                    child.wait()
                })
                .is_ok_and(|s| s.success());
            log(serde_json::json!({ "event": "hooked", "pid": pid, "pane": pane, "session": session, "ok": ok }));
        }
    }

    println!("fake {kind} ready");
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        log(serde_json::json!({ "event": "input", "pid": pid, "pane": pane, "line": line }));
        if line.trim() == "/exit" {
            break;
        }
        println!("got: {line}");
    }
}
