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
//!   A message containing `!draft` leaves `leftover` in its input box
//!   after its turn, the cursor after it, as a box no one typed into.
//!   A message containing `!ask` raises a permission prompt instead (a
//!   `permission_prompt` notification) and leaves it up. `/exit` exits.
//!   `!size` (as either) prints its terminal's size, `stty size`'s rows and
//!   columns: the size whatever draws the pane last gave its PTY. `!menu`
//!   (as either) draws a numbered menu as its last lines, `❯ 1. Yes`, and
//!   leaves it up with no hook: a dialog like Codex's update prompt.
//!
//! Ready for a message, either draws its input box as the real ones do,
//! `❯ ` as `claude` and `› ` as `codex`, Pi's and OpenCode's boxes as
//! `pi` and `opencode` — Mirador waits for one before it types. `claude`'s
//! carries a dim placeholder with the cursor before it, as Claude Code's
//! does. It edits its line itself, as the real ones do: what is typed
//! shows after the glyph at normal intensity, clearing the placeholder,
//! with the cursor after it; Home or Ctrl+A moves the cursor to the start
//! of the line, End or Ctrl+E back to its end, Backspace deletes before
//! it; Enter sends the line, and a message pasted into a draft is sent
//! with it, as the real ones did (#244). With `E2E_WELCOME=1` it starts on a welcome screen instead,
//! with no input box until its first input, which only dismisses it: as
//! Codex's logo waits for a key before its login menu.
//!   Its transcript, `~/.claude/projects/fake/<session>.jsonl`, named to
//!   Mirador by its hooks as Claude Code's are, gets each turn's prompt
//!   and one Bash call, `echo` of the message's first line, with its
//!   result — a failure (exit 1) when the message says `!fail` — then its
//!   reply, `replied to: ` and that line. The call's
//!   reply is Opus 5.5's and carries a usage, the same each turn
//!   ([`TURN_USAGE`]), for a cost to be read from it.
//!
//! As `codex` it echoes each message, and holds a rollout open as Codex
//! does (`~/.codex/sessions/YYYY/MM/DD/rollout-….jsonl`), writing each
//! message there as a turn with one `echo` command, which lasts
//! `E2E_TURN_MS`: meanwhile it shows Codex's status line, `• Working (0s •
//! esc to interrupt)`, over its input box. `/new` starts a new
//! rollout in the same process, as Codex's does. With
//! `E2E_CODEX_NO_ROLLOUT=1` it holds none, as no process table can show
//! Mirador on Windows. Started with a `-c notify=[…]`, it runs that
//! program after each turn with Codex's JSON appended, the turn's thread
//! in it, and its reply, `replied to: ` and the message's first line.
//!
//! As `pi` or `opencode` it is a `codex` with no rollout and no notify
//! of its own: it logs how it was started (with `OPENCODE_CONFIG_CONTENT`,
//! where an OpenCode role's model and prompt are), and echoes each
//! message.

use std::io::Write;
use std::time::Duration;

/// What `claude` shows in its empty input box, as Claude Code's
/// `Try "how does styles.css work?"`.
const PLACEHOLDER: &str = "Try \"fake\"";

/// The line being typed, edited as a harness's input box edits it, its
/// terminal out of canonical mode and echo (`stty`), so a key reaches it
/// at once and it draws what is typed itself.
struct LineEditor {
    input: std::io::Bytes<std::io::StdinLock<'static>>,
    line: Vec<char>,
    /// Where the cursor is in `line`.
    at: usize,
}

impl LineEditor {
    fn new() -> Self {
        let _ = std::process::Command::new("stty").args(["-icanon", "-echo", "min", "1"]).status();
        LineEditor { input: std::io::Read::bytes(std::io::stdin().lock()), line: Vec::new(), at: 0 }
    }

    fn byte(&mut self) -> Option<u8> {
        self.input.next()?.ok()
    }

    /// The next line sent with Enter, a bracketed paste's newlines in it;
    /// `None` once the terminal is gone.
    fn next_line(&mut self) -> Option<String> {
        let mut pasting = false;
        let mut utf8 = Vec::new();
        loop {
            let b = self.byte()?;
            match b {
                0x1b => match self.escape()?.as_str() {
                    "[200~" => pasting = true,
                    "[201~" => pasting = false,
                    "[H" | "[1~" | "OH" => self.home(),
                    "[F" | "[4~" | "OF" => self.end(),
                    // Focus and mouse reports, replies, other keys.
                    _ => {}
                },
                b'\n' if !pasting => {
                    print!("\r\n");
                    let _ = std::io::stdout().flush();
                    self.at = 0;
                    return Some(std::mem::take(&mut self.line).into_iter().collect());
                }
                0x01 if !pasting => self.home(),
                0x05 if !pasting => self.end(),
                0x7f | 0x08 if !pasting => self.backspace(),
                _ => {
                    utf8.push(b);
                    if let Ok(text) = std::str::from_utf8(&utf8) {
                        let text = text.to_string();
                        utf8.clear();
                        for c in text.chars() {
                            self.insert(c);
                        }
                    } else if utf8.len() >= 4 {
                        utf8.clear();
                    }
                }
            }
            let _ = std::io::stdout().flush();
        }
    }

    /// What follows an ESC: a CSI up to its final byte, or SS3 and one.
    fn escape(&mut self) -> Option<String> {
        let mut seq = String::new();
        match self.byte()? {
            b'[' => {
                seq.push('[');
                loop {
                    let b = self.byte()?;
                    seq.push(b as char);
                    if (0x40..=0x7e).contains(&b) {
                        return Some(seq);
                    }
                }
            }
            b'O' => Some(format!("O{}", self.byte()? as char)),
            b => Some((b as char).to_string()),
        }
    }

    fn insert(&mut self, c: char) {
        if self.line.is_empty() {
            // Typing clears the placeholder.
            print!("\x1b[K");
        }
        if c == '\n' {
            print!("\r\n");
        } else {
            let rest: String = self.line[self.at..].iter().collect();
            print!("{c}{rest}");
            if !rest.is_empty() {
                print!("\x1b[{}D", rest.chars().count());
            }
        }
        self.line.insert(self.at, c);
        self.at += 1;
    }

    fn backspace(&mut self) {
        if self.at == 0 {
            return;
        }
        self.at -= 1;
        self.line.remove(self.at);
        let rest: String = self.line[self.at..].iter().collect();
        print!("\x08{rest} \x1b[{}D", rest.chars().count() + 1);
    }

    fn home(&mut self) {
        if self.at > 0 {
            print!("\x1b[{}D", self.at);
            self.at = 0;
        }
    }

    fn end(&mut self) {
        let left = self.line.len() - self.at;
        if left > 0 {
            print!("\x1b[{left}C");
            self.at = self.line.len();
        }
    }
}

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
/// writes: the prompt, a Bash call, its result, and the reply ending it.
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
        serde_json::json!({ "type": "assistant", "timestamp": at, "message": {
            "id": format!("msg_{}_{n}_reply", std::process::id()),
            "role": "assistant",
            "content": [{ "type": "text", "text": format!("replied to: {first}") }],
            "stop_reason": "end_turn",
        } }),
    ];
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(text.as_bytes());
    }
}

/// Opens the `seq`th rollout of this process, as Codex does: in today's
/// directory, named for when it began and for its thread — a UUID made of
/// the pid and `seq`, as `codex resume` takes one — its session first.
fn open_rollout(seq: u32) -> Option<std::fs::File> {
    if std::env::var("E2E_CODEX_NO_ROLLOUT").is_ok_and(|v| v == "1") {
        return None;
    }
    let home = std::path::PathBuf::from(std::env::var("HOME").ok()?);
    let at = timestamp();
    let dir = home.join(".codex/sessions").join(at[..10].replace('-', "/"));
    std::fs::create_dir_all(&dir).ok()?;
    let name = format!("rollout-{}-{}.jsonl", at[..19].replace(':', "-"), thread(seq));
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(name)).ok()?;
    let meta = serde_json::json!({ "timestamp": at, "type": "session_meta", "payload": { "id": format!("fake-{seq}"), "cli_version": "fake" } });
    file.write_all(format!("{meta}\n").as_bytes()).ok()?;
    Some(file)
}

/// The `seq`th thread of this process.
fn thread(seq: u32) -> String {
    format!("{:08x}-0000-4000-8000-{seq:012x}", std::process::id())
}

/// Runs the `notify` this Codex was started with (`-c notify=[…]`) as
/// Codex does after a turn: its JSON appended, not waited for.
fn notify(args: &[String], seq: u32, n: u32, message: &str) {
    let Some(argv) = args
        .windows(2)
        .filter(|w| w[0] == "-c" && w[1].starts_with("notify="))
        .find_map(|w| cmux_core::agents::codex_user_notify(Some(&w[1]), None))
    else {
        return;
    };
    let payload = serde_json::json!({
        "type": "agent-turn-complete",
        "thread-id": thread(seq),
        "turn-id": format!("turn-{n}"),
        "cwd": std::env::current_dir().ok(),
        "input-messages": [message],
        "last-assistant-message": format!("replied to: {}", message.lines().next().unwrap_or_default()),
    });
    let spawned = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .arg(payload.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    log(serde_json::json!({ "event": "notify", "pid": std::process::id(), "argv": argv, "ok": spawned.is_ok() }));
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

/// The settings file it was started with (`--settings`), read once.
static SETTINGS: std::sync::OnceLock<Option<serde_json::Value>> = std::sync::OnceLock::new();

/// Runs the hook for `event` (`session-start`, …) with `payload`, as
/// Claude Code does: through `sh`, the command its `--settings` names for
/// that event — what Mirador gives an agent on a remote — else `mira
/// claude-hook <event>` (`E2E_MIRA`), as the hooks a person sets up here
/// call. Whether it succeeded.
fn hook(event: &str, payload: serde_json::Value) -> bool {
    let name = match event {
        "session-start" => "SessionStart",
        "prompt-submit" => "UserPromptSubmit",
        "stop" => "Stop",
        "notification" => "Notification",
        _ => "",
    };
    let named = SETTINGS.get().and_then(Option::as_ref).and_then(|s| {
        s["hooks"][name].as_array()?.iter().find_map(|m| m["hooks"][0]["command"].as_str().map(String::from))
    });
    let mut command = match named {
        Some(line) => {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", &line]);
            c
        }
        None => {
            let Ok(mira) = std::env::var("E2E_MIRA") else { return false };
            let mut c = std::process::Command::new(mira);
            c.args(["claude-hook", event]);
            c
        }
    };
    command
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
    // The name without its extension: `claude.exe` on Windows is claude.
    let kind = std::path::Path::new(&argv0)
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let args: Vec<String> = args.collect();
    let _ = SETTINGS.set(
        args.iter()
            .position(|a| a == "--settings")
            .and_then(|i| std::fs::read_to_string(args.get(i + 1)?).ok())
            .and_then(|t| serde_json::from_str(&t).ok()),
    );
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
        "socket": std::env::var("MIRA_SOCKET").ok(),
        // What an OpenCode role is given its model and prompt in.
        "opencodeConfig": std::env::var("OPENCODE_CONFIG_CONTENT").ok(),
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
    // Each as the real one draws it, for Mirador's screen checks
    // (`handoff::showing_in`): Pi's box between two rules over its footer,
    // OpenCode's ending in its `╹▀` edge — and, as the real ones leave it,
    // the cursor back up inside the box (`ESC [ n A`), with rows drawn
    // below it that a screen read must still see.
    let input_box = || {
        let rule = "─".repeat(40);
        match kind.as_str() {
            "pi" => print!("{rule}\n\n{rule}\n~/proj\n0.0%/128k (auto)    fake\n\x1b[4A\r"),
            "opencode" => print!("  ┃\n  ┃  Build · Fake\n  ╹{}\n\x1b[3A\r  ┃  ", "▀".repeat(40)),
            // Claude Code's placeholder, dim after `❯ `, the cursor back
            // at its start: an empty box whose text is not empty.
            _ if claude => print!("❯ \x1b[2m{PLACEHOLDER}\x1b[22m\x1b[{}D", PLACEHOLDER.len()),
            _ => print!("› "),
        }
        let _ = std::io::stdout().flush();
    };
    let mut welcome = std::env::var("E2E_WELCOME").is_ok_and(|v| v == "1");
    if welcome {
        println!("  Welcome to fake {kind}\n  Press any key");
    } else {
        input_box();
    }
    let mut turns = 0;
    let mut sessions = 1;
    let mut rollout = if kind == "codex" { open_rollout(sessions) } else { None };
    let mut editor = LineEditor::new();
    while let Some(message) = editor.next_line() {
        if message.trim().is_empty() {
            continue;
        }
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
            if message.trim() == "/new" {
                sessions += 1;
                rollout = open_rollout(sessions);
            } else {
                turns += 1;
                if kind == "codex" {
                    // As Codex at work: its status line over its input
                    // box, which stays up for a message to steer the turn.
                    print!("• Working (0s • esc to interrupt)\r\n\r\n");
                    input_box();
                }
                if let Some(file) = rollout.as_mut() {
                    rollout_turn(file, turns, &message, turn);
                }
                notify(&args, sessions, turns, &message);
                print!("\r\n");
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
        if message.contains("!draft") {
            // Text in its box, the cursor after it, that no one typed.
            print!("leftover");
            let _ = std::io::stdout().flush();
        }
    }
    log(serde_json::json!({ "event": "exit", "pid": pid, "pane": pane }));
}
