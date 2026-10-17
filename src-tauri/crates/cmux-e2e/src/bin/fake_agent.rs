//! A stand-in for `claude` and `codex` in end-to-end tests: linked into the
//! sandbox's PATH under those names, so Mirador's process table sees a
//! `claude` (or `codex`) on the pane's terminal, exactly as for the real
//! one — a shell script would show up as `/bin/sh`.
//!
//! Every event goes to `E2E_AGENT_LOG`, one JSON line each. As `claude` it
//! behaves enough like Claude Code for Mirador to treat it as one:
//!
//! - `-p --output-format stream-json` (the skills probe a limited role
//!   makes): prints an `init` event listing `E2E_SKILLS` (comma-separated)
//!   and one plugin, then waits to be killed, as the real one is;
//! - otherwise it records its session through the real `mira claude-hook
//!   session-start` (`E2E_MIRA`) — resuming `--resume`'s, or a new one —
//!   and reads its terminal. A bracketed paste is one message; a message
//!   is a turn: `prompt-submit`, a moment's work (`E2E_TURN_MS`), `stop`.
//!   A message containing `!ask` raises a permission prompt instead (a
//!   `permission_prompt` notification) and leaves it up. `/exit` exits.

use std::io::{BufRead, Write};
use std::time::Duration;

/// Appends `event` as one line. One `write_all` of the whole line: the
/// team's agents start at once and share the file, and `writeln!` may write
/// in pieces that interleave with another process's.
fn log(event: serde_json::Value) {
    let Ok(path) = std::env::var("E2E_AGENT_LOG") else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(format!("{event}\n").as_bytes());
    }
}

/// Runs `mira claude-hook <event>` with `payload`, as Claude Code's hooks
/// do; whether it succeeded.
fn hook(event: &str, payload: serde_json::Value) -> bool {
    let Ok(mira) = std::env::var("E2E_MIRA") else { return false };
    std::process::Command::new(mira)
        .args(["claude-hook", event])
        .env_remove("CLAUDE_JOB_DIR")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            child.stdin.take().unwrap().write_all(payload.to_string().as_bytes())?;
            child.wait()
        })
        .is_ok_and(|s| s.success())
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
    let has = |a: &str| args.iter().any(|x| x == a);

    if kind == "claude" && has("-p") && has("stream-json") {
        let skills: Vec<String> = std::env::var("E2E_SKILLS")
            .unwrap_or_else(|_| "dataviz,loop,anthropic-skills:pdf".into())
            .split(',')
            .map(str::to_string)
            .collect();
        log(serde_json::json!({ "event": "probe", "kind": kind, "pid": pid, "args": args, "cwd": std::env::current_dir().ok() }));
        println!(
            "{}",
            serde_json::json!({
                "type": "system",
                "subtype": "init",
                "skills": skills,
                "plugins": [
                    { "name": "rust", "path": "/plugins/rust", "source": "rust@market" },
                    { "name": "cc-plugin-telemetry", "path": "builtin", "source": "cc-plugin-telemetry@builtin" }
                ],
                "mcp_servers": []
            })
        );
        let _ = std::io::stdout().flush();
        // The real one goes on to call a model; Mirador kills it first.
        std::thread::sleep(Duration::from_secs(60));
        return;
    }

    log(serde_json::json!({
        "event": "start",
        "kind": kind,
        "pid": pid,
        "args": args,
        "pane": pane,
        "cwd": std::env::current_dir().ok(),
    }));

    let claude = kind == "claude";
    let session = args
        .iter()
        .position(|a| a == "--resume")
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(|| {
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
            format!("fake-{pid}-{}", t.as_millis())
        });
    if claude {
        let ok = hook("session-start", serde_json::json!({ "session_id": session }));
        log(serde_json::json!({ "event": "hooked", "pid": pid, "pane": pane, "session": session, "ok": ok }));
    }
    let turn = Duration::from_millis(
        std::env::var("E2E_TURN_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(800),
    );

    println!("fake {kind} ready");
    let mut paste: Option<Vec<String>> = None;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        // A bracketed paste spans lines: gather it into one message.
        let message = if let Some(rest) = line.strip_prefix("\x1b[200~") {
            match rest.split_once("\x1b[201~") {
                Some((all, _)) => Some(all.to_string()),
                None => {
                    paste = Some(vec![rest.to_string()]);
                    None
                }
            }
        } else if let Some(lines) = paste.as_mut() {
            match line.split_once("\x1b[201~") {
                Some((last, _)) => {
                    lines.push(last.to_string());
                    Some(paste.take().unwrap().join("\n"))
                }
                None => {
                    lines.push(line.clone());
                    None
                }
            }
        } else {
            Some(line.clone())
        };
        let Some(message) = message.filter(|m| !m.trim().is_empty()) else { continue };
        log(serde_json::json!({ "event": "message", "pid": pid, "pane": pane, "text": message }));
        if message.trim() == "/exit" {
            break;
        }
        if !claude {
            println!("got: {message}");
            continue;
        }
        if message.contains("!ask") {
            hook(
                "notification",
                serde_json::json!({
                    "session_id": session,
                    "notification_type": "permission_prompt",
                    "message": "Claude needs your permission to use Bash"
                }),
            );
            println!("? allow Bash (fake permission prompt)");
            continue;
        }
        hook("prompt-submit", serde_json::json!({ "session_id": session }));
        std::thread::sleep(turn);
        println!("done: {}", message.lines().next().unwrap_or_default());
        hook("stop", serde_json::json!({ "session_id": session }));
        log(serde_json::json!({ "event": "turn-done", "pid": pid, "pane": pane }));
    }
    log(serde_json::json!({ "event": "exit", "pid": pid, "pane": pane }));
}
