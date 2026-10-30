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
//!   `!size` (as either) prints its terminal's size, `stty size`'s rows and
//!   columns: the size whatever draws the pane last gave its PTY. `!menu`
//!   (as either) draws a numbered menu as its last lines, `❯ 1. Yes`, and
//!   leaves it up with no hook: a dialog like Codex's update prompt.
//!
//! Ready for a message, either draws its input box as the real ones do,
//! `❯ ` as `claude` and `› ` as `codex` — Mirador waits for one before it
//! types. With `E2E_WELCOME=1` it starts on a welcome screen instead,
//! with no input box until its first input, which only dismisses it: as
//! Codex's logo waits for a key before its login menu.
//!   Its transcript, `~/.claude/projects/fake/<session>.jsonl`, named to
//!   Mirador by its hooks as Claude Code's are, gets each turn's prompt
//!   and one Bash call, `echo` of the message's first line, with its
//!   result — a failure (exit 1) when the message says `!fail`. The call's
//!   reply is Opus 5.5's and carries a usage, the same each turn
//!   ([`TURN_USAGE`]), for a cost to be read from it.
//!
//! As `codex` it echoes each message, and holds a rollout open as Codex
//! does (`~/.codex/sessions/YYYY/MM/DD/rollout-….jsonl`), writing each
//! message there as a turn with one `echo` command. `/new` starts a new
//! rollout in the same process, as Codex's does.

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

/// What each turn's reply used: 10 tokens in, 100 written to the cache,
/// 1000 read from it, 20 out — $0.00114 at Opus 5.5's list prices.
const TURN_USAGE: &str = r#"{"input_tokens":10,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"output_tokens":20}"#;

/// Appends one turn to the transcript at `path`, in the shape Claude Code
/// writes: the prompt, a Bash call, and its result.
fn transcript_turn(path: &std::path::Path, n: u32, message: &str) {
    let first = message.lines().next().unwrap_or_default();
    let id = format!("toolu_{}_{n}", std::process::id());
    let at = timestamp();
    let lines = [
        serde_json::json!({ "type": "user", "timestamp": at, "message": { "role": "user", "content": message } }),
        serde_json::json!({ "type": "assistant", "timestamp": at, "message": {
            "id": format!("msg_{}_{n}", std::process::id()),
            "role": "assistant",
            "model": "claude-opus-5-5",
            "content": [{ "type": "tool_use", "id": id, "name": "Bash", "input": { "command": command(first) } }],
            "usage": serde_json::from_str::<serde_json::Value>(TURN_USAGE).unwrap_or_default(),
        } }),
        serde_json::json!({ "type": "user", "timestamp": at, "message": { "role": "user", "content": [
            { "type": "tool_result", "tool_use_id": id, "content": result(first), "is_error": failing(first) }
        ] } }),
    ];
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(text.as_bytes());
    }
}

/// Opens the `seq`th rollout of this process, as Codex does: in today's
/// directory, named for when it began, its session first.
fn open_rollout(seq: u32) -> Option<std::fs::File> {
    let home = std::path::PathBuf::from(std::env::var("HOME").ok()?);
    let at = timestamp();
    let dir = home.join(".codex/sessions").join(at[..10].replace('-', "/"));
    std::fs::create_dir_all(&dir).ok()?;
    let name = format!("rollout-{}-{}-{seq}.jsonl", at[..19].replace(':', "-"), std::process::id());
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(name)).ok()?;
    let meta = serde_json::json!({ "timestamp": at, "type": "session_meta", "payload": { "id": format!("fake-{seq}"), "cli_version": "fake" } });
    file.write_all(format!("{meta}\n").as_bytes()).ok()?;
    Some(file)
}

/// Writes one turn to a rollout: its start and prompt, `took` of work,
/// then an `echo` of its first line as the one command it ran, and its end.
fn rollout_turn(file: &mut std::fs::File, n: u32, message: &str, took: Duration) {
    let first = message.lines().next().unwrap_or_default();
    let (at, turn) = (timestamp(), format!("turn-{n}"));
    let event = |payload: serde_json::Value| serde_json::json!({ "timestamp": at, "type": "event_msg", "payload": payload });
    let mut write = |lines: &[serde_json::Value]| {
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
        let _ = file.write_all(text.as_bytes());
    };
    write(&[
        event(serde_json::json!({ "type": "task_started", "turn_id": turn })),
        event(serde_json::json!({ "type": "item_completed", "turn_id": turn, "item": {
            "type": "UserMessage", "id": format!("u-{n}"), "content": [{ "type": "text", "text": message }] } })),
    ]);
    std::thread::sleep(took);
    write(&[
        event(serde_json::json!({ "type": "item_completed", "turn_id": turn, "item": {
            "type": "CommandExecution", "id": format!("cmd-{}-{n}", std::process::id()),
            "command": ["/bin/zsh", "-lc", command(first)],
            "status": if failing(first) { "failed" } else { "completed" },
            "exit_code": i32::from(failing(first)), "aggregated_output": first } })),
        event(serde_json::json!({ "type": "task_complete", "turn_id": turn })),
    ]);
}

/// A turn's one command: an `echo` of its message's first line, which
/// fails (exit 1) when the message says `!fail`.
fn failing(first: &str) -> bool {
    first.contains("!fail")
}

fn command(first: &str) -> String {
    if failing(first) { format!("echo {first} && false") } else { format!("echo {first}") }
}

/// Its result as Claude Code writes it: a failed command's output headed
/// with its exit code.
fn result(first: &str) -> String {
    if failing(first) { format!("Exit code 1\n{first}") } else { first.to_string() }
}

/// Now, as Claude Code writes it: `2026-10-25T09:00:00.000Z`.
fn timestamp() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let (days, rem) = (ms.div_euclid(86_400_000), ms.rem_euclid(86_400_000));
    // The civil date of a day count (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let (h, min, sec, milli) = (rem / 3_600_000, rem / 60_000 % 60, rem / 1000 % 60, rem % 1000);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{sec:02}.{milli:03}Z")
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
    let transcript = std::env::var("HOME")
        .map(|home| std::path::PathBuf::from(home).join(".claude/projects/fake").join(format!("{session}.jsonl")))
        .ok()
        .filter(|_| claude);
    if let Some(dir) = transcript.as_deref().and_then(std::path::Path::parent) {
        let _ = std::fs::create_dir_all(dir);
    }
    if claude {
        let ok = hook(
            "session-start",
            serde_json::json!({ "session_id": session, "transcript_path": transcript }),
        );
        log(serde_json::json!({ "event": "hooked", "pid": pid, "pane": pane, "session": session, "ok": ok }));
    }
    let turn = Duration::from_millis(
        std::env::var("E2E_TURN_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(800),
    );

    println!("fake {kind} ready");
    let input_box = || {
        print!("{} ", if claude { '❯' } else { '›' });
        let _ = std::io::stdout().flush();
    };
    let mut welcome = std::env::var("E2E_WELCOME").is_ok_and(|v| v == "1");
    if welcome {
        println!("  Welcome to fake {kind}\n  Press any key");
    } else {
        input_box();
    }
    let mut paste: Option<Vec<String>> = None;
    let mut turns = 0;
    let mut sessions = 1;
    let mut rollout = if kind == "codex" { open_rollout(sessions) } else { None };
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
        if welcome {
            welcome = false;
            input_box();
            continue;
        }
        if message.trim() == "!size" {
            let _ = std::process::Command::new("stty").arg("size").status();
            input_box();
            continue;
        }
        if message.contains("!menu") {
            println!("  Pick one\n  ❯ 1. Yes\n    2. No");
            continue;
        }
        if !claude {
            if message.trim() == "/new" && rollout.is_some() {
                sessions += 1;
                rollout = open_rollout(sessions);
            } else if let Some(file) = rollout.as_mut() {
                turns += 1;
                rollout_turn(file, turns, &message, turn);
            }
            println!("got: {message}");
            input_box();
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
        if let Some(path) = &transcript {
            turns += 1;
            transcript_turn(path, turns, &message);
        }
        std::thread::sleep(turn);
        println!("done: {}", message.lines().next().unwrap_or_default());
        hook("stop", serde_json::json!({ "session_id": session }));
        log(serde_json::json!({ "event": "turn-done", "pid": pid, "pane": pane }));
        input_box();
    }
    log(serde_json::json!({ "event": "exit", "pid": pid, "pane": pane }));
}
