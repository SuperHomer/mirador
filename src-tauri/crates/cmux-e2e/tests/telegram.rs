//! The Telegram bridge, end to end: the debug app pointed at a fake Bot
//! API (`MIRA_TELEGRAM_API`), a fake `claude` behind it, the real `mira
//! telegram` CLI in front. Skipped unless `MIRA_E2E=1`.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use cmux_e2e::{e2e, Sandbox};
use cmux_protocol::Request;
use serde_json::{json, Value};

const LONG: Duration = Duration::from_secs(45);
/// How long the fake holds a `getUpdates` with nothing to hand out.
const HOLD: Duration = Duration::from_secs(1);

/// What the fake Bot API knows and saw.
#[derive(Default)]
struct World {
    /// Tokens `getMe` accepts.
    known: HashSet<String>,
    /// Tokens `getUpdates` answers 401 for.
    revoked: HashSet<String>,
    updates: Vec<Value>,
    next_id: i64,
    /// Each `getUpdates`: its offset, and when.
    polls: Vec<(Option<i64>, Instant)>,
    /// Each `sendMessage`: its chat and text.
    sent: Vec<(i64, String)>,
}

/// A Bot API on a local port: just enough of `getMe`, `getUpdates` and
/// `sendMessage`, one thread a connection.
#[derive(Clone)]
struct FakeTelegram {
    url: String,
    world: Arc<(Mutex<World>, Condvar)>,
}

impl FakeTelegram {
    fn start(tokens: &[&str]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let world = World { known: tokens.iter().map(|t| t.to_string()).collect(), next_id: 100, ..Default::default() };
        let fake = Self { url, world: Arc::new((Mutex::new(world), Condvar::new())) };
        let server = fake.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let server = server.clone();
                std::thread::spawn(move || server.serve(stream));
            }
        });
        fake
    }

    /// Requests on one connection, kept alive as reqwest keeps it.
    fn serve(&self, stream: TcpStream) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut stream = stream;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let path = line.split_whitespace().nth(1).unwrap_or_default().to_string();
            let mut length = 0;
            loop {
                let mut header = String::new();
                if reader.read_line(&mut header).unwrap_or(0) == 0 {
                    return;
                }
                let header = header.trim();
                if header.is_empty() {
                    break;
                }
                if let Some((name, value)) = header.split_once(':') {
                    if name.eq_ignore_ascii_case("content-length") {
                        length = value.trim().parse().unwrap_or(0);
                    }
                }
            }
            let mut body = vec![0; length];
            if reader.read_exact(&mut body).is_err() {
                return;
            }
            let body: Value = serde_json::from_slice(&body).unwrap_or_default();
            let (status, answer) = self.answer(&path, &body);
            let answer = answer.to_string();
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{answer}",
                answer.len()
            );
            if stream.write_all(response.as_bytes()).is_err() {
                return;
            }
        }
    }

    fn answer(&self, path: &str, body: &Value) -> (&'static str, Value) {
        let unauthorized = ("401 Unauthorized", json!({ "ok": false, "error_code": 401, "description": "Unauthorized" }));
        let Some((token, method)) = path.strip_prefix("/bot").and_then(|p| p.split_once('/')) else {
            return ("404 Not Found", json!({ "ok": false, "error_code": 404, "description": "Not Found" }));
        };
        let (lock, wake) = &*self.world;
        let mut world = lock.lock().unwrap();
        if !world.known.contains(token) {
            return unauthorized;
        }
        match method {
            "getMe" => ("200 OK", json!({ "ok": true, "result": { "id": 1, "is_bot": true, "username": "fake_bot" } })),
            "getUpdates" => {
                if world.revoked.contains(token) {
                    return unauthorized;
                }
                let offset = body["offset"].as_i64();
                world.polls.push((offset, Instant::now()));
                let deadline = Instant::now() + HOLD;
                loop {
                    let due: Vec<Value> =
                        world.updates.iter().filter(|u| offset.is_none_or(|o| u["update_id"].as_i64().unwrap() >= o)).cloned().collect();
                    let left = deadline.saturating_duration_since(Instant::now());
                    if !due.is_empty() || left.is_zero() {
                        return ("200 OK", json!({ "ok": true, "result": due }));
                    }
                    world = wake.wait_timeout(world, left).unwrap().0;
                }
            }
            "sendMessage" => {
                world.sent.push((body["chat_id"].as_i64().unwrap_or_default(), body["text"].as_str().unwrap_or_default().into()));
                ("200 OK", json!({ "ok": true, "result": { "message_id": 1 } }))
            }
            _ => ("404 Not Found", json!({ "ok": false, "error_code": 404, "description": "Not Found" })),
        }
    }

    /// Queues a text message from `chat`, and gives its update id.
    fn say(&self, chat: i64, private: bool, text: &str) -> i64 {
        let (lock, wake) = &*self.world;
        let mut world = lock.lock().unwrap();
        world.next_id += 1;
        let id = world.next_id;
        let kind = if private { "private" } else { "group" };
        world.updates.push(json!({
            "update_id": id,
            "message": { "message_id": id, "chat": { "id": chat, "type": kind }, "text": text },
        }));
        wake.notify_all();
        id
    }

    /// Whether the app asked for updates past `id`: it has handled it.
    fn handled(&self, id: i64) -> bool {
        self.world.0.lock().unwrap().polls.iter().any(|(offset, _)| offset.is_some_and(|o| o > id))
    }

    fn sent_to(&self, chat: i64) -> Vec<String> {
        self.world.0.lock().unwrap().sent.iter().filter(|(c, _)| *c == chat).map(|(_, t)| t.clone()).collect()
    }

    fn polls(&self) -> usize {
        self.world.0.lock().unwrap().polls.len()
    }

    /// Whether nothing has polled for a while: longer than a poll is held.
    fn quiet(&self) -> bool {
        let world = self.world.0.lock().unwrap();
        world.polls.last().is_none_or(|(_, at)| at.elapsed() > HOLD * 3)
    }

    fn revoke(&self, token: &str) {
        self.world.0.lock().unwrap().revoked.insert(token.into());
    }
}

/// A sandbox talking to `fake`, launched, with a plain fake `claude`
/// beside its first pane: the shell and the agent.
fn sandbox(name: &str, fake: &FakeTelegram) -> (Sandbox, String, String) {
    let mut sb = Sandbox::new(name);
    sb.set_env("E2E_TURN_MS", "300");
    sb.set_env("MIRA_TELEGRAM_API", &fake.url);
    sb.launch();
    let shell = sb.first_pane();
    let agent = new_agent(&sb, &shell);
    (sb, shell, agent)
}

fn new_agent(sb: &Sandbox, from: &str) -> String {
    let agent = sb
        .request(Request::AgentNew { role: None, harness: None, title: None, task: None, target: None, pane_id: Some(from.into()), host: None, dir: None, on: None })
        .map(|v| v["paneId"].as_str().unwrap().to_string())
        .unwrap();
    sb.wait_for("its session", LONG, || sb.events("hooked", &agent).into_iter().next());
    agent
}

/// `mira telegram connect <agent>` from `from`, the token in the
/// environment as a person would give it: exit code, stdout, stderr.
fn connect(sb: &Sandbox, from: &str, agent: &str, token: &str) -> (i32, String, String) {
    let out = sb
        .mira_command(Some(from), &["telegram", "connect", agent])
        .env("MIRA_TELEGRAM_TOKEN", token)
        .output()
        .unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into(), String::from_utf8_lossy(&out.stderr).into())
}

/// The pairing code `connect` printed.
fn code_of(out: &str) -> String {
    out.split("/start ").nth(1).and_then(|r| r.split_whitespace().next()).unwrap_or_else(|| panic!("no code in {out}")).into()
}

fn bridges(sb: &Sandbox) -> Vec<Value> {
    sb.call(Request::TelegramStatus)["bridges"].as_array().cloned().unwrap_or_default()
}

/// Every file of the sandbox's that holds `token`: the app's log and its
/// session among them. Links are not followed: they lead to the binaries.
fn files_holding(dir: &Path, token: &str, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            files_holding(&entry.path(), token, found);
        } else if kind.is_file() {
            let bytes = std::fs::read(entry.path()).unwrap_or_default();
            if bytes.windows(token.len()).any(|w| w == token.as_bytes()) {
                found.push(entry.path().display().to_string());
            }
        }
    }
}

/// The token in no file the app wrote: its log, its session, anything.
fn assert_token_nowhere(sb: &mut Sandbox, token: &str) {
    sb.quit();
    let session = ["Library/Application Support/Mirador", ".local/share/mirador"]
        .iter()
        .map(|d| sb.home.join(d).join("session.json"))
        .find(|p| p.exists())
        .expect("the session was saved");
    assert!(sb.root.join("app.log").exists());
    let mut found = Vec::new();
    files_holding(&sb.root, token, &mut found);
    assert!(found.is_empty(), "the token is in {found:?}");
    assert!(!std::fs::read_to_string(session).unwrap().contains(token));
}

#[test]
fn only_the_paired_chat_reaches_the_agent_and_hears_back() {
    e2e!();
    let token = "1111:pair-AAAAsecret";
    let fake = FakeTelegram::start(&[token]);
    let (mut sb, shell, agent) = sandbox("tg-pair", &fake);

    let (code, out, err) = connect(&sb, &shell, &agent, token);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("in a private chat"), "{out}");
    assert!(!out.contains(token) && !err.contains(token));
    let pairing = code_of(&out);

    // A stranger who found the bot, a wrong code, the right one in a group.
    let (stranger, guesser, group, phone) = (111, 222, -333, 444);
    fake.say(stranger, true, "stranger says hi");
    fake.say(guesser, true, "/start 000000000000");
    let last = fake.say(group, false, &format!("/start {pairing}"));
    sb.wait_for("the app to read them", LONG, || fake.handled(last).then_some(()));
    let in_group = fake.say(group, false, "group says hi");
    sb.wait_for("the app to read that too", LONG, || fake.handled(in_group).then_some(()));
    assert_eq!(bridges(&sb)[0]["paired"], false);

    fake.say(phone, true, &format!("/start {pairing}"));
    let hello = sb.wait_for("the pairing's answer", LONG, || fake.sent_to(phone).into_iter().next());
    assert!(hello.starts_with("Connected to "), "{hello}");
    assert_eq!(bridges(&sb)[0]["paired"], true);

    fake.say(phone, true, "hello from the phone");
    sb.wait_for("the message to reach the agent", LONG, || {
        sb.events("message", &agent).into_iter().find(|m| m["text"].as_str().is_some_and(|t| t.ends_with("hello from the phone")))
    });
    let reply = sb.wait_for("the turn's end on the phone", LONG, || {
        fake.sent_to(phone).into_iter().find(|t| t.contains("replied to: hello from the phone"))
    });
    assert!(reply.ends_with(":\n\nreplied to: hello from the phone"), "{reply}");

    // Nothing of the others ever reached the agent, or was answered.
    for message in sb.events("message", &agent) {
        let text = message["text"].as_str().unwrap_or_default();
        assert!(!text.contains("says hi") && !text.contains("/start"), "{text}");
    }
    for chat in [stranger, guesser, group] {
        assert!(fake.sent_to(chat).is_empty(), "chat {chat} heard {:?}", fake.sent_to(chat));
    }
    assert_token_nowhere(&mut sb, token);
}

#[test]
fn one_bot_bridges_one_agent() {
    e2e!();
    let token = "2222:one-BBBBsecret";
    let fake = FakeTelegram::start(&[token]);
    let (mut sb, shell, first) = sandbox("tg-one", &fake);
    let second = new_agent(&sb, &shell);

    let (code, _, err) = connect(&sb, &shell, &first, token);
    assert_eq!(code, 0, "{err}");
    let name = bridges(&sb)[0]["agent"].as_str().unwrap().to_string();
    let (code, _, err) = connect(&sb, &shell, &second, token);
    assert_ne!(code, 0);
    assert!(err.contains("already bridged to") && err.contains(&name), "{err}");
    assert!(!err.contains(token), "{err}");
    // The same agent connecting again replaces its bridge.
    let (code, _, err) = connect(&sb, &shell, &first, token);
    assert_eq!(code, 0, "{err}");
    let listed = bridges(&sb);
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0]["paneId"], first.as_str());
    assert_token_nowhere(&mut sb, token);
}

#[test]
fn a_bridge_ends_with_disconnect_and_with_its_pane() {
    e2e!();
    let token = "3333:end-CCCCsecret";
    let fake = FakeTelegram::start(&[token]);
    let (mut sb, shell, agent) = sandbox("tg-end", &fake);

    let (code, _, err) = connect(&sb, &shell, &agent, token);
    assert_eq!(code, 0, "{err}");
    let polled = fake.polls();
    sb.wait_for("it to poll", LONG, || (fake.polls() > polled).then_some(()));
    let (code, _, err) = sb.mira(Some(&shell), &["telegram", "disconnect", &agent]);
    assert_eq!(code, 0, "{err}");
    assert!(bridges(&sb).is_empty());
    sb.wait_for("polling to stop", LONG, || fake.quiet().then_some(()));
    let (_, out, _) = sb.mira(Some(&shell), &["telegram", "status"]);
    assert!(out.contains("no Telegram bridges"), "{out}");

    let (code, _, err) = connect(&sb, &shell, &agent, token);
    assert_eq!(code, 0, "{err}");
    sb.wait_for("it to poll again", LONG, || (!fake.quiet()).then_some(()));
    sb.call(Request::ClosePane { pane_id: agent.clone() });
    sb.wait_for("the bridge to go with its pane", LONG, || bridges(&sb).is_empty().then_some(()));
    sb.wait_for("polling to stop", LONG, || fake.quiet().then_some(()));
    assert_token_nowhere(&mut sb, token);
}

#[test]
fn a_revoked_token_ends_the_bridge() {
    e2e!();
    let token = "4444:revoked-DDDDsecret";
    let fake = FakeTelegram::start(&[token]);
    let (mut sb, shell, agent) = sandbox("tg-revoked", &fake);

    // A token Telegram never knew fails at once.
    let (code, _, err) = connect(&sb, &shell, &agent, "9999:unknown-EEEEsecret");
    assert_ne!(code, 0);
    assert!(err.contains("Unauthorized") && !err.contains("EEEEsecret"), "{err}");

    let (code, _, err) = connect(&sb, &shell, &agent, token);
    assert_eq!(code, 0, "{err}");
    assert_eq!(bridges(&sb).len(), 1);
    fake.revoke(token);
    sb.wait_for("the bridge to end", LONG, || bridges(&sb).is_empty().then_some(()));
    sb.wait_for("polling to stop", LONG, || fake.quiet().then_some(()));
    assert_token_nowhere(&mut sb, token);
}

/// Connects `agent` to `token` and pairs the private chat `phone` with it.
fn pair(sb: &Sandbox, fake: &FakeTelegram, from: &str, agent: &str, token: &str, phone: i64) {
    let (code, out, err) = connect(sb, from, agent, token);
    assert_eq!(code, 0, "{err}");
    fake.say(phone, true, &format!("/start {}", code_of(&out)));
    sb.wait_for("the pairing's answer", LONG, || {
        fake.sent_to(phone).into_iter().find(|t| t.starts_with("Connected to "))
    });
}

/// A permission prompt with no tool call in the transcript to show still
/// reaches the phone, in the hook's own words.
#[test]
fn a_permission_prompt_reaches_the_phone() {
    e2e!();
    let token = "5555:ask-FFFFsecret";
    let fake = FakeTelegram::start(&[token]);
    let (mut sb, shell, agent) = sandbox("tg-ask", &fake);
    let phone = 555;
    pair(&sb, &fake, &shell, &agent, token, phone);

    fake.say(phone, true, "deploy it, !ask first");
    let asked = sb.wait_for("the prompt on the phone", LONG, || {
        fake.sent_to(phone).into_iter().find(|t| t.contains("permission"))
    });
    assert!(asked.ends_with(": Claude needs your permission to use Bash"), "{asked}");
    assert_token_nowhere(&mut sb, token);
}

/// A Codex's reply comes from its notify's payload, not a transcript.
#[test]
fn a_codex_turns_reply_reaches_the_phone() {
    e2e!();
    let token = "6666:codex-GGGGsecret";
    let fake = FakeTelegram::start(&[token]);
    let mut sb = Sandbox::new("tg-codex");
    sb.set_env("E2E_TURN_MS", "300");
    sb.set_env("MIRA_TELEGRAM_API", &fake.url);
    sb.launch();
    let shell = sb.first_pane();
    let agent = sb
        .request(Request::AgentNew {
            role: None,
            harness: Some(cmux_protocol::Harness::Codex),
            title: None,
            task: None,
            target: None,
            pane_id: Some(shell.clone()),
            host: None,
            dir: None,
            on: None,
        })
        .map(|v| v["paneId"].as_str().unwrap().to_string())
        .unwrap();
    sb.nth_start(&agent, 1);
    let phone = 666;
    pair(&sb, &fake, &shell, &agent, token, phone);

    fake.say(phone, true, "hello codex");
    let reply = sb.wait_for("the turn's end on the phone", LONG, || {
        fake.sent_to(phone).into_iter().find(|t| t.contains("replied to"))
    });
    assert!(reply.ends_with(":\n\nreplied to: hello codex"), "{reply}");
    assert_token_nowhere(&mut sb, token);
}
