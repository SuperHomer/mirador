//! The real Pi and OpenCode, on a local model: Qwen 3.5 0.8B under
//! `llama-server`, so they cost nothing and need no login. The debug app,
//! as the ordinary e2e tests run; only `pi` and `opencode` are the real
//! ones in the sandbox's PATH, with a `node` new enough for Pi.
//!
//! Skipped unless `MIRA_E2E=1` and `MIRA_E2E_LOCAL=1`, and when `pi`,
//! `opencode`, `node`, `llama-server` or the model is missing. The model is
//! `MIRA_E2E_MODEL`, by default `~/models/Qwen3.5-0.8B-Q8_0.gguf`.
//!
//! OpenCode runs only as a role here. A plain OpenCode talks to the per-user
//! background service, on a fixed port that the person's own OpenCode
//! already holds; a role's runs `--standalone`, on a server of its own.
//!
//! Only what a 0.8B model does reliably is asked of it: to say a word it
//! was given. Replies are read from each harness's own store, never the
//! screen: Pi's session files, and OpenCode's sessions as `opencode
//! session export` gives them.

// The sandbox symlinks the person's node, fd and rg into its HOME, and the
// suite runs on macOS only: none of this builds on Windows.
#![cfg(unix)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use cmux_e2e::{on_path, Sandbox};
use cmux_protocol::Request;
use serde_json::Value;

/// A small model at work, thinking aloud first.
const REPLY: Duration = Duration::from_secs(240);
const LONG: Duration = Duration::from_secs(45);

/// The model the tests run, from `MIRA_E2E_MODEL`.
fn model() -> PathBuf {
    std::env::var_os("MIRA_E2E_MODEL").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("models/Qwen3.5-0.8B-Q8_0.gguf")
    })
}

/// The first `node` on the PATH that Pi runs on: 22.19 or later (its
/// `engines`). Pi's launcher prefers a Node its installer put in the home
/// (`~/.local/share/pi-node`), which a sandbox's home does not have, and
/// otherwise takes the first on the pane's PATH — where macOS's
/// `path_helper` puts `/usr/local/bin` first, and with it whatever old
/// `node` is there.
fn node() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|d| d.join("node")).filter(|n| n.is_file()).find(|n| {
        let out = Command::new(n).arg("--version").output().ok();
        let version = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().trim_start_matches('v').to_string());
        let parts: Vec<u32> = version.unwrap_or_default().split('.').filter_map(|p| p.parse().ok()).collect();
        matches!(parts[..], [major, minor, ..] if (major, minor) >= (22, 19))
    })
}

/// Why these tests cannot run here, if they cannot.
fn missing() -> Option<String> {
    if !cmux_e2e::enabled() || std::env::var("MIRA_E2E_LOCAL").ok().as_deref() != Some("1") {
        return Some("set MIRA_E2E=1 and MIRA_E2E_LOCAL=1 to run the real Pi and OpenCode on a local model".into());
    }
    for tool in ["pi", "opencode", "llama-server"] {
        if on_path(tool).is_none() {
            return Some(format!("no `{tool}` on the PATH"));
        }
    }
    if node().is_none() {
        return Some("no `node` of 22.19 or later on the PATH, which Pi needs".into());
    }
    let model = model();
    (!model.is_file()).then(|| format!("no model at {} (set MIRA_E2E_MODEL)", model.display()))
}

/// `llama-server` on a port of its own, its log in the sandbox; killed when
/// dropped, a failed test included.
struct Llama {
    child: Child,
    port: u16,
}

impl Llama {
    fn start(log: &Path) -> Llama {
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let log = std::fs::File::create(log).unwrap();
        let child = Command::new(on_path("llama-server").unwrap())
            .arg("-m")
            .arg(model())
            .args(["--host", "127.0.0.1", "--port", &port.to_string(), "--jinja"])
            // Thinking off: a 0.8B model can think for minutes, or for
            // ever, before it answers — or answer with thoughts alone.
            .args(["--reasoning", "off"])
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .expect("start llama-server");
        let llama = Llama { child, port };
        let deadline = Instant::now() + Duration::from_secs(120);
        while !llama.healthy() {
            assert!(Instant::now() < deadline, "llama-server did not come up on {port}");
            std::thread::sleep(Duration::from_millis(250));
        }
        llama
    }

    /// `GET /health` answers `ok` once the model is loaded.
    fn healthy(&self) -> bool {
        let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", self.port)) else { return false };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let _ = stream.write_all(b"GET /health HTTP/1.0\r\nHost: localhost\r\n\r\n");
        let mut answer = String::new();
        let _ = stream.read_to_string(&mut answer);
        answer.starts_with("HTTP/1.1 200") && answer.contains("\"ok\"")
    }
}

impl Drop for Llama {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A sandbox running the real Pi and OpenCode on the local model. The
/// sandbox goes first, then the model it talked to.
struct Local {
    sb: Sandbox,
    _llama: Llama,
}

impl std::ops::Deref for Local {
    type Target = Sandbox;
    fn deref(&self) -> &Sandbox {
        &self.sb
    }
}

impl std::ops::DerefMut for Local {
    fn deref_mut(&mut self) -> &mut Sandbox {
        &mut self.sb
    }
}

/// The sandbox `name`, set up as a person would for a local model: Pi's
/// `models.json` and OpenCode's provider, both on the server's
/// OpenAI-compatible endpoint, as `local/qwen`. None, saying why, when the
/// tests cannot run here.
fn local(name: &str) -> Option<Local> {
    if let Some(why) = missing() {
        eprintln!("skipped: {why}");
        return None;
    }
    let sb = Sandbox::new(name);
    let llama = Llama::start(&sb.root.join("llama.log"));
    let endpoint = format!("http://127.0.0.1:{}/v1", llama.port);
    let bin = sb.root.join("bin");
    for tool in ["pi", "opencode"] {
        let _ = std::fs::remove_file(bin.join(tool));
        std::os::unix::fs::symlink(on_path(tool).unwrap(), bin.join(tool)).unwrap();
    }
    // Where Pi's launcher looks for its own Node first.
    let pi_node = sb.home.join(".local/share/pi-node/current/bin");
    std::fs::create_dir_all(&pi_node).unwrap();
    std::os::unix::fs::symlink(node().unwrap(), pi_node.join("node")).unwrap();
    let pi = sb.home.join(".pi/agent");
    std::fs::create_dir_all(pi.join("bin")).unwrap();
    std::fs::write(
        pi.join("models.json"),
        serde_json::json!({ "providers": { "local": {
            "baseUrl": endpoint, "api": "openai-completions", "apiKey": "none", "models": [{ "id": "qwen" }]
        } } })
        .to_string(),
    )
    .unwrap();
    // No tools, for Pi as for OpenCode below: given them, the 0.8B model
    // goes off editing files instead of answering, and is still at it when
    // the reply is waited for. An empty `defaultTools` turns off every
    // built-in one.
    std::fs::write(pi.join("settings.json"), serde_json::json!({ "defaultTools": [] }).to_string()).unwrap();
    // Pi fetches `fd` and `rg` into its own `bin` on a first start, and takes
    // no message until it has: the person's, when there are, spare that.
    let theirs = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".pi/agent/bin");
    for tool in ["fd", "rg"] {
        if theirs.join(tool).is_file() {
            std::os::unix::fs::symlink(theirs.join(tool), pi.join("bin").join(tool)).unwrap();
        }
    }
    // Under the sandbox's XDG_CONFIG_HOME, which OpenCode reads rather
    // than `~/.config`. Not found, `local/qwen` names no provider, and
    // OpenCode quietly puts the role on its own hosted default model.
    let opencode_dir = sb.config.join("opencode");
    std::fs::create_dir_all(&opencode_dir).unwrap();
    std::fs::write(
        opencode_dir.join("opencode.json"),
        // The local provider alone: without `enabled_providers`, a model
        // OpenCode cannot find goes to its own hosted ones instead. No
        // tools: given them, the 0.8B model calls one again and again
        // instead of answering; without, it answers.
        serde_json::json!({
            "provider": { "local": {
                "npm": "@ai-sdk/openai-compatible", "name": "Local",
                "options": { "baseURL": endpoint, "apiKey": "none" },
                "models": { "qwen": { "name": "Qwen" } }
            } },
            "enabled_providers": ["local"],
            "tools": { "*": false }
        })
        .to_string(),
    )
    .unwrap();
    // A config OpenCode does not read leaves it its hosted models, and a
    // test could pass on one of them: none starts unless the local model
    // is all it has.
    let models = opencode(&sb, &["models", "--standalone"]);
    assert_eq!(models.trim(), "local/qwen", "OpenCode's models, as the sandbox sets it up");
    Some(Local { sb, _llama: llama })
}

/// Starts the role `role` beside `from`, and returns its pane.
fn start(sb: &Sandbox, role: &str, from: &str) -> String {
    sb.call(Request::AgentNew {
        role: Some(role.into()),
        harness: None,
        title: None,
        task: None,
        target: None,
        pane_id: Some(from.into()),
        host: None,
        dir: None,
        on: None,
    })["paneId"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Waits until `pane`'s screen shows its harness is up ([`pi_up`],
/// [`opencode_up`]).
fn ready(sb: &Sandbox, pane: &str, what: &str, up: fn(&str) -> bool) {
    sb.wait_for_screen(pane, &format!("{what} up in {pane}"), LONG, up);
}

/// Sends `message` to `to` with `mira agent send`, from `from`, once
/// Mirador's look at the agent's screen lets it through: no input box yet
/// while it starts, or a dialog over it, is refused, and tried again.
fn send(sb: &Sandbox, from: &str, to: &str, message: &str) {
    sb.wait_for(&format!("a send to {to}"), LONG, || {
        let (code, _, _) = sb.mira(Some(from), &["agent", "send", to, message]);
        if code == 0 {
            Some(())
        } else {
            std::thread::sleep(Duration::from_secs(1));
            None
        }
    });
}

/// Pi is up: its header, and the role's prompt file listed as context.
fn pi_up(screen: &str) -> bool {
    screen.contains("[Context]")
}

/// OpenCode is up once its input box names the model (`┃ Qwen`), not as
/// soon as the box is drawn. A prompt submitted before it has a model is
/// not sent: OpenCode opens `Connect an integration` over the box instead
/// ("Connect an integration to send prompts"), and the message is lost.
/// An answer's own line (`Helper · Qwen · 2.5 tok/s`) is not the box's,
/// so a resumed conversation is no false start.
fn opencode_up(screen: &str) -> bool {
    screen.lines().any(|l| l.trim_start().starts_with('┃') && l.contains("Qwen"))
}

/// One turn of a conversation as a harness stored it.
#[derive(Debug, Clone)]
struct Turn {
    user: bool,
    text: String,
}

/// Every Pi conversation in the sandbox, each as its turns, by session id.
fn pi_conversations(sb: &Sandbox) -> Vec<(String, Vec<Turn>)> {
    let root = sb.home.join(".pi/agent/sessions");
    let mut out = Vec::new();
    for dir in std::fs::read_dir(&root).into_iter().flatten().flatten() {
        for file in std::fs::read_dir(dir.path()).into_iter().flatten().flatten() {
            let Ok(text) = std::fs::read_to_string(file.path()) else { continue };
            let mut id = String::new();
            let mut turns = Vec::new();
            for line in text.lines() {
                let Ok(entry) = serde_json::from_str::<Value>(line) else { continue };
                if entry["type"] == "session" {
                    id = entry["id"].as_str().unwrap_or_default().to_string();
                }
                let message = &entry["message"];
                if entry["type"] != "message" || !matches!(message["role"].as_str(), Some("user" | "assistant")) {
                    continue;
                }
                turns.push(Turn { user: message["role"] == "user", text: texts(&message["content"]) });
            }
            out.push((id, turns));
        }
    }
    out
}

/// The text parts of a message's content: a string, or a list of parts of
/// which those of type `text` count.
fn texts(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|p| p["type"] == "text")
            .filter_map(|p| p["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// `opencode` run as the sandbox's own, on a private server, in the
/// project: what it printed.
fn opencode(sb: &Sandbox, args: &[&str]) -> String {
    let out = Command::new(on_path("opencode").unwrap())
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", &sb.home)
        .env("XDG_CONFIG_HOME", &sb.config)
        .env("TERM", "dumb")
        .current_dir(&sb.project)
        .stdin(Stdio::null())
        .output()
        .expect("run opencode");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Every OpenCode conversation in the sandbox's project, each as its turns,
/// by session id: what `opencode session export` gives.
fn opencode_conversations(sb: &Sandbox) -> Vec<(String, Vec<Turn>)> {
    let list: Value = serde_json::from_str(&opencode(sb, &["session", "list", "--standalone", "--format", "json"]))
        .unwrap_or(Value::Null);
    let ids: Vec<String> =
        list.as_array().into_iter().flatten().filter_map(|s| s["id"].as_str().map(String::from)).collect();
    ids.into_iter()
        .map(|id| {
            let export: Value =
                serde_json::from_str(&opencode(sb, &["session", "export", "--standalone", &id])).unwrap_or(Value::Null);
            // `{"type": "user", "text": …}`, `{"type": "assistant", "content":
            // [parts]}`, and others (`idle`, …) that are no one's turn.
            let turns = export["messages"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| match m["type"].as_str()? {
                    "user" => Some(Turn { user: true, text: m["text"].as_str().unwrap_or_default().to_string() }),
                    "assistant" => Some(Turn { user: false, text: texts(&m["content"]) }),
                    _ => None,
                })
                .collect();
            (id, turns)
        })
        .collect()
}

/// The conversations a harness keeps, as [`pi_conversations`] and
/// [`opencode_conversations`] read them.
type Store = fn(&Sandbox) -> Vec<(String, Vec<Turn>)>;

/// Waits for a reply that `answers` after a user turn containing `asked`,
/// in one of the conversations `read` gives, and gives that conversation's
/// id and turns. Run out, it fails with what the store held and what
/// `pane`, the agent's, showed: a reply that never came is told apart
/// from one that went elsewhere.
fn wait_turn(
    sb: &Sandbox,
    what: &str,
    pane: &str,
    read: Store,
    asked: &str,
    answers: impl Fn(&str) -> bool,
) -> (String, Vec<Turn>) {
    let deadline = Instant::now() + REPLY;
    loop {
        let conversations = read(sb);
        let found = conversations.iter().find(|(_, turns)| {
            let Some(at) = turns.iter().rposition(|t| t.user && t.text.contains(asked)) else { return false };
            turns[at..].iter().any(|t| !t.user && answers(&t.text))
        });
        if let Some(found) = found {
            return found.clone();
        }
        if Instant::now() >= deadline {
            let screen = sb.screen(pane).unwrap_or_default();
            sb.fail(&format!("no {what} in {REPLY:?}; its store holds {conversations:#?}; its pane shows:\n{screen}"));
        }
        std::thread::sleep(Duration::from_secs(3));
    }
}

/// [`wait_turn`] for a reply containing `word`, in any case: `Heron` is
/// the codeword `HERON`.
fn wait_reply(sb: &Sandbox, what: &str, pane: &str, read: Store, asked: &str, word: &str) -> (String, Vec<Turn>) {
    wait_turn(sb, what, pane, read, asked, |text| text.to_lowercase().contains(&word.to_lowercase()))
}

/// Each role, told a codeword in its prompt, says it when asked — read
/// from its harness's own store.
#[test]
fn pi_and_opencode_roles_answer_from_their_prompt() {
    let Some(mut sb) = local("lmcode") else { return };
    sb.write(
        ".mirador/roles/builder.md",
        "---\nharness: pi\nmodel: local/qwen\n---\nYour codeword is PELICAN. When asked for the codeword, answer with it.\n",
    );
    sb.write(
        ".mirador/roles/helper.md",
        "---\nharness: opencode\nmodel: local/qwen\n---\nYour codeword is HERON. When asked for the codeword, answer with it.\n",
    );
    sb.launch();
    let first = sb.first_pane();
    let builder = start(&sb, "builder", &first);
    let helper = start(&sb, "helper", &first);
    ready(&sb, &builder, "Pi", pi_up);
    ready(&sb, &helper, "OpenCode", opencode_up);
    let ask = "What is your codeword? Answer with the codeword only.";
    send(&sb, &first, &builder, ask);
    send(&sb, &first, &helper, ask);
    wait_reply(&sb, "Pi's codeword", &builder, pi_conversations, "your codeword", "PELICAN");
    wait_reply(&sb, "OpenCode's codeword", &helper, opencode_conversations, "your codeword", "HERON");
}

/// [`wait_turn`] for any reply at all: the model's own words.
fn wait_answer(sb: &Sandbox, what: &str, pane: &str, read: Store, asked: &str) {
    wait_turn(sb, what, pane, read, asked, |text| !text.trim().is_empty());
}

/// A team of a Pi lead and an OpenCode member: each is sent a message
/// from the other's pane, gets it with the `[message from …]` header that
/// says who it is from, and answers. The sends are the test's: the 0.8B
/// model does not run `mira agent send` itself reliably.
#[test]
fn a_pi_and_opencode_team_take_messages_from_each_other() {
    let Some(mut sb) = local("lmteam") else { return };
    sb.write(".mirador/roles/builder.md", "---\nharness: pi\nmodel: local/qwen\n---\nYou build. Answer every message briefly.\n");
    sb.write(".mirador/roles/helper.md", "---\nharness: opencode\nmodel: local/qwen\n---\nYou help. Answer every message briefly.\n");
    sb.write(
        ".mirador/teams/duo.md",
        "---\nmembers:\n  - role: builder\n    lead: true\n  - role: helper\n---\nShip small.\n",
    );
    sb.launch();
    let first = sb.first_pane();
    sb.call(Request::TeamStart { title: None, team: "duo".into(), task: None, pane_id: Some(first), host: None, dir: None, on: None });
    let pane = |name: &str| sb.wait_for(name, LONG, || sb.member(name).and_then(|m| m["pane"].as_str().map(String::from)));
    let (builder, helper) = (pane("builder"), pane("helper"));
    ready(&sb, &builder, "Pi", pi_up);
    ready(&sb, &helper, "OpenCode", opencode_up);
    send(&sb, &builder, "helper", "Say hello to the builder.");
    send(&sb, &helper, "builder", "Say hello to the helper.");
    wait_answer(&sb, "the helper's answer", &helper, opencode_conversations, "[message from builder");
    wait_answer(&sb, "the builder's answer", &builder, pi_conversations, "[message from helper");
}

/// A Pi role and an OpenCode role are told a number; the app quits, ending
/// them, and comes back; each is typed back on its conversation and still
/// knows the number, in the same conversation as it was told.
#[test]
fn pi_and_opencode_roles_remember_across_a_restart() {
    let Some(mut sb) = local("lmresume") else { return };
    sb.write(".mirador/roles/builder.md", "---\nharness: pi\nmodel: local/qwen\n---\nYou build. Answer briefly.\n");
    sb.write(".mirador/roles/helper.md", "---\nharness: opencode\nmodel: local/qwen\n---\nYou help. Answer briefly.\n");
    sb.launch();
    let first = sb.first_pane();
    let builder = start(&sb, "builder", &first);
    let helper = start(&sb, "helper", &first);
    ready(&sb, &builder, "Pi", pi_up);
    ready(&sb, &helper, "OpenCode", opencode_up);
    let told = "Remember this: the secret number is 4721. Reply OK.";
    send(&sb, &first, &builder, told);
    send(&sb, &first, &helper, told);
    wait_answer(&sb, "Pi's OK", &builder, pi_conversations, "secret number is 4721");
    wait_answer(&sb, "OpenCode's OK", &helper, opencode_conversations, "secret number is 4721");

    sb.quit();
    sb.launch();
    let first = sb.first_pane();
    ready(&sb, &builder, "Pi", pi_up);
    ready(&sb, &helper, "OpenCode", opencode_up);
    let ask = "What is the secret number I told you? Answer with the number only.";
    send(&sb, &first, &builder, ask);
    send(&sb, &first, &helper, ask);
    for (who, pane, read) in [("Pi", &builder, pi_conversations as Store), ("OpenCode", &helper, opencode_conversations)] {
        let (_, turns) = wait_reply(&sb, &format!("{who}'s recall"), pane, read, "secret number I told you", "4721");
        assert!(turns.iter().any(|t| t.user && t.text.contains("secret number is 4721")), "{who}: told in another conversation: {turns:?}");
    }
}

/// The `pi:` record the session file holds for a pane, once it is saved.
fn saved_pi_session(sb: &Sandbox) -> Option<String> {
    let file = ["Library/Application Support/Mirador", ".local/share/mirador"]
        .iter()
        .find_map(|d| std::fs::read_to_string(sb.home.join(d).join("session.json")).ok())?;
    let at = file.find("\"pi:")? + "\"pi:".len();
    Some(file[at..].split('"').next()?.to_string())
}

/// A Pi started by hand in a pane, not by Mirador, reports its session and
/// turns through the extension `mira hooks setup` installs: its session
/// recorded for resume, the pane idle after the turn, and the turn's end
/// notified.
#[test]
fn a_pi_started_by_hand_reports_its_turns_through_the_extension() {
    let Some(mut sb) = local("lmpihook") else { return };
    let (code, out, err) = sb.mira(None, &["hooks", "setup"]);
    assert_eq!(code, 0, "{out}{err}");
    assert!(out.contains("installed the Pi extension"), "{out}");
    sb.launch();
    let first = sb.first_pane();
    let pane = sb.call(Request::SplitPane { pane_id: Some(first.clone()), dir: cmux_protocol::SplitDir::Row, command: None })
        ["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    sb.type_into(&pane, "pi --model local/qwen\n");
    // Looking at another pane, so its notification is not read on arrival.
    sb.call(Request::FocusPane { pane_id: first.clone() });

    send(&sb, &first, &pane, "Say hello.");
    let (id, _) = wait_turn(&sb, "Pi's answer", &pane, pi_conversations, "Say hello.", |text| !text.trim().is_empty());
    let status = |sb: &Sandbox| {
        let agents = sb.call(Request::AgentList)["agents"].as_array().cloned().unwrap_or_default();
        agents.iter().find(|a| a["paneId"] == pane.as_str()).map(|a| a["status"].clone())
    };
    sb.wait_for("the turn's end notified", LONG, || {
        sb.snapshot()["unreadPanes"].as_array().unwrap().iter().any(|p| p == pane.as_str()).then_some(())
    });
    assert_eq!(status(&sb), Some(Value::from("idle")));
    let saved = sb.wait_for("its session in the session file", LONG, || saved_pi_session(&sb));
    assert_eq!(saved, id);
}
