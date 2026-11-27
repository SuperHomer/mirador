//! A Telegram bot bridged to an agent pane: the chat paired with it types
//! into the agent, and hears when a turn ends or a permission prompt comes.
//! `cmux_core::telegram` decides what each update means and what is sent;
//! this does the HTTP and the threads.
//!
//! A bridge lives in memory only, and its token with it: not in the
//! session, the config or a log. Every error that could name the token
//! (an HTTP error names its URL, which holds it) goes through `redact`.
//! So a bridge ends with its pane, a new connect, a disconnect, or the
//! app: a relaunch starts with none.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cmux_core::telegram::{self as core, Decision, Pairing};
use tauri::{AppHandle, Manager};

use crate::AppState;

/// How long one `getUpdates` waits for a message, in seconds.
const POLL_SECS: u64 = 25;
/// The longest wait after a network error before trying again.
const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// The bridges, by the pane of their agent.
pub type Bridges = Mutex<HashMap<String, Arc<Bridge>>>;

pub struct Bridge {
    api: Api,
    pairing: Mutex<Pairing>,
    /// What is sent to the chat, in order, by the bridge's sending
    /// thread, which ends once the bridge is dropped.
    outbox: Sender<Out>,
}

/// What the sending thread is asked to send.
pub enum Out {
    Say(String),
    TurnEnded { transcript: Option<PathBuf>, reply: Option<String> },
    /// The call it asks about, when the transcript shows it, and the
    /// hook's own message.
    Asking { asking: Option<cmux_protocol::Asking>, message: Option<String> },
}

/// Bridges `pane`'s agent to the bot `token` names: checks the token, then
/// polls in the background. Answers the pairing code and the bot's username.
pub fn connect(app: &AppHandle, pane: String, token: String) -> Result<(String, String), String> {
    let state = app.state::<AppState>();
    let other = state.telegram.lock().unwrap().iter().find(|(p, b)| **p != pane && b.api.token == token).map(|(p, _)| p.clone());
    if let Some(other) = other {
        // Two polls on one bot steal each other's messages.
        return Err(format!("that bot is already bridged to {}", label(&state, &other)));
    }
    let api = Api::new(token)?;
    let me = api.call("getMe", serde_json::json!({}), Duration::from_secs(15)).map_err(|f| f.message)?;
    let bot = me["username"].as_str().unwrap_or_default().to_string();
    let (outbox, inbox) = channel();
    let bridge = Arc::new(Bridge { api, pairing: Mutex::new(Pairing::new()), outbox });
    let code = bridge.pairing.lock().unwrap().code().to_string();
    // Replacing a bridge ends the old one's threads: see `current`.
    state.telegram.lock().unwrap().insert(pane.clone(), bridge.clone());
    let (a, p, b) = (app.clone(), pane.clone(), bridge.clone());
    std::thread::spawn(move || poll(a, p, b));
    let (a, p, api) = (app.clone(), pane, bridge.api.clone());
    let chat = Arc::downgrade(&bridge);
    std::thread::spawn(move || send_all(a, p, api, chat, inbox));
    Ok((code, bot))
}

pub fn disconnect(state: &AppState, pane: &str) -> Result<(), String> {
    state.telegram.lock().unwrap().remove(pane).map(|_| ()).ok_or_else(|| format!("no Telegram bridge on {pane}"))
}

pub fn status(state: &AppState) -> Vec<cmux_protocol::TelegramBridge> {
    let bridges: Vec<(String, bool)> = state
        .telegram
        .lock()
        .unwrap()
        .iter()
        .map(|(pane, b)| (pane.clone(), b.pairing.lock().unwrap().chat().is_some()))
        .collect();
    let mut out: Vec<_> = bridges
        .into_iter()
        .map(|(pane, paired)| cmux_protocol::TelegramBridge { agent: label(state, &pane), pane_id: pane, paired })
        .collect();
    out.sort_by(|a, b| a.pane_id.cmp(&b.pane_id));
    out
}

/// Tells `pane`'s bridge, if it has a paired one, about its agent.
pub fn tell(state: &AppState, pane: &str, out: Out) {
    let bridges = state.telegram.lock().unwrap();
    if let Some(bridge) = bridges.get(pane).filter(|b| b.pairing.lock().unwrap().chat().is_some()) {
        let _ = bridge.outbox.send(out);
    }
}

/// The agent's name, as the bridge's messages name it.
fn label(state: &AppState, pane: &str) -> String {
    crate::handoff::sender(state, pane).unwrap_or_else(|| pane.to_string())
}

/// Whether `bridge` is still `pane`'s: once it is not, its threads end.
fn current(state: &AppState, pane: &str, bridge: &Arc<Bridge>) -> bool {
    state.telegram.lock().unwrap().get(pane).is_some_and(|b| Arc::ptr_eq(b, bridge))
}

/// Long-polls for messages until the bridge is no longer the pane's,
/// waiting longer after each failure in a row.
fn poll(app: AppHandle, pane: String, bridge: Arc<Bridge>) {
    let state = &*app.state::<AppState>();
    let mut offset: Option<i64> = None;
    let mut backoff = Duration::from_secs(1);
    while current(state, &pane, &bridge) {
        let mut body = serde_json::json!({ "timeout": POLL_SECS, "allowed_updates": ["message"] });
        if let Some(offset) = offset {
            body["offset"] = offset.into();
        }
        let updates = bridge
            .api
            .call("getUpdates", body, Duration::from_secs(POLL_SECS + 10))
            .map(|result| core::updates(&result));
        let updates = match updates {
            Ok(updates) => updates,
            Err(failure) => {
                eprintln!("mirador: Telegram bridge of {pane}: {}", failure.message);
                if failure.revoked {
                    // The token no longer works, and never will again.
                    eprintln!("mirador: Telegram bridge of {pane} ended: its token was refused");
                    let mut bridges = state.telegram.lock().unwrap();
                    if bridges.get(&pane).is_some_and(|b| Arc::ptr_eq(b, &bridge)) {
                        bridges.remove(&pane);
                    }
                    return;
                }
                // In seconds, so a disconnect does not wait out the backoff.
                for _ in 0..backoff.as_secs() {
                    std::thread::sleep(Duration::from_secs(1));
                    if !current(state, &pane, &bridge) {
                        return;
                    }
                }
                backoff = (backoff * 2).min(MAX_BACKOFF);
                continue;
            }
        };
        backoff = Duration::from_secs(1);
        // Disconnected while the poll waited: what it brought is not ours.
        if !current(state, &pane, &bridge) {
            return;
        }
        offset = core::next_offset(&updates).or(offset);
        for update in &updates {
            let decision = bridge.pairing.lock().unwrap().decide(update);
            let say = match decision {
                Decision::Paired(_) => Some(format!("Connected to {}", label(state, &pane))),
                Decision::Input(text) => match crate::handoff::send(&app, &pane, &text, None, false, None, None) {
                    Ok((_, Some(queued))) => Some(format!("(queued: {})", queued.why)),
                    Ok((_, None)) => None,
                    Err(e) => Some(e),
                },
                Decision::Ignore => None,
            };
            if let Some(say) = say {
                let _ = bridge.outbox.send(Out::Say(say));
            }
        }
    }
}

/// Sends what the bridge is asked to, in order, to its paired chat. Holds
/// the bridge weakly, so it ends with it: once no one holds the bridge, the
/// outbox's sender is gone and `recv` fails. A message that fails to send
/// is reported and the next still goes.
fn send_all(app: AppHandle, pane: String, api: Api, bridge: std::sync::Weak<Bridge>, inbox: Receiver<Out>) {
    let state = &*app.state::<AppState>();
    while let Ok(out) = inbox.recv() {
        let Some(chat) = bridge.upgrade().and_then(|b| b.pairing.lock().unwrap().chat()) else { continue };
        let texts = match out {
            Out::Say(text) => core::chunks(&text),
            Out::TurnEnded { transcript, reply } => {
                let reply = reply.or_else(|| transcript.and_then(|t| core::last_reply(&t)));
                core::turn_ended(&label(state, &pane), reply.as_deref())
            }
            Out::Asking { asking, message } => core::asking(&label(state, &pane), asking.as_ref(), message.as_deref()),
        };
        for text in texts {
            let body = serde_json::json!({ "chat_id": chat, "text": text });
            if let Err(failure) = api.call("sendMessage", body, Duration::from_secs(15)) {
                eprintln!("mirador: Telegram bridge of {pane}: {}", failure.message);
            }
        }
    }
}

/// The Bot API for one token.
#[derive(Clone)]
struct Api {
    base: String,
    token: String,
    client: reqwest::blocking::Client,
}

impl Api {
    fn new(token: String) -> Result<Self, String> {
        crate::whatsnew::ensure_crypto_provider();
        // Only a debug build can be pointed elsewhere: in a release one, no
        // environment can send the token to another host.
        let elsewhere = cfg!(debug_assertions).then(|| std::env::var("MIRA_TELEGRAM_API").ok()).flatten();
        let base = elsewhere.filter(|b| !b.is_empty()).unwrap_or_else(|| "https://api.telegram.org".into());
        let base = base.trim_end_matches('/').to_string();
        let client = reqwest::blocking::Client::builder().build().map_err(|e| e.to_string())?;
        Ok(Self { base, token, client })
    }

    /// `method`'s result, or why there is none, never naming the token.
    fn call(&self, method: &str, body: serde_json::Value, timeout: Duration) -> Result<serde_json::Value, Failure> {
        let url = format!("{}/bot{}/{method}", self.base, self.token);
        let fail = |revoked, e: String| Failure { revoked, message: core::redact(&e, &self.token) };
        let response = self
            .client
            .post(url)
            .timeout(timeout)
            .json(&body)
            .send()
            .map_err(|e| fail(false, format!("could not reach Telegram: {}", e.without_url())))?;
        let revoked = response.status() == reqwest::StatusCode::UNAUTHORIZED;
        // Telegram explains a refusal in the body, whatever the status.
        let text = response.text().map_err(|e| fail(revoked, format!("could not read Telegram's answer: {}", e.without_url())))?;
        core::answer(&text).map_err(|e| fail(revoked, format!("Telegram: {e}")))
    }
}

/// Why a call failed: `revoked` when Telegram no longer knows the token.
struct Failure {
    revoked: bool,
    message: String,
}
