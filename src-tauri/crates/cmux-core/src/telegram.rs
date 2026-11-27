//! The Telegram bridge's logic: what a `getUpdates` answer holds, which
//! chat may reach the agent, and what is sent back. No HTTP here — the app
//! does the polling and the sending, and hands every answer to this.
//!
//! A bridge pairs with one chat: the first private chat that sends
//! `/start <code>`, the code `mira telegram connect` printed. Before that
//! nothing reaches the agent, and after it only that chat does. Anyone can
//! find a bot by its name and write to it, so a stranger, a wrong code and
//! a group (whose every member would then type into the agent) are all
//! ignored.

use std::io::BufRead;
use std::path::Path;

use serde_json::Value;

use crate::transcript::text;

/// The most a Telegram message holds, in UTF-16 code units, which is how
/// Telegram counts: an emoji is two.
pub const MESSAGE_LIMIT: usize = 4096;

/// One update from `getUpdates`. `chat` and `text` are None for anything
/// but a text message (a photo, a member joining, an edit), which still
/// has to be counted past.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub id: i64,
    pub chat: Option<i64>,
    /// A one-to-one chat with the bot, rather than a group or a channel.
    pub private: bool,
    pub text: Option<String>,
}

/// The `result` of a Telegram answer, or Telegram's own description of
/// why it refused (`ok: false`).
pub fn answer(body: &str) -> Result<Value, String> {
    let mut json: Value = serde_json::from_str(body).map_err(|e| format!("not a Telegram answer: {e}"))?;
    if json["ok"].as_bool() != Some(true) {
        return Err(text(&json["description"]).unwrap_or_else(|| "Telegram refused the request".into()));
    }
    Ok(json["result"].take())
}

/// The updates in a `getUpdates` answer.
pub fn parse_updates(body: &str) -> Result<Vec<Update>, String> {
    Ok(updates(&answer(body)?))
}

/// The updates in a `getUpdates` result.
pub fn updates(result: &Value) -> Vec<Update> {
    let updates = result.as_array().map(Vec::as_slice).unwrap_or(&[]);
    updates
        .iter()
        .filter_map(|u| {
            let message = &u["message"];
            let chat = &message["chat"];
            Some(Update {
                id: u["update_id"].as_i64()?,
                chat: chat["id"].as_i64(),
                private: chat["type"] == "private",
                text: text(&message["text"]),
            })
        })
        .collect()
}

/// The `offset` the next `getUpdates` asks from, which also tells
/// Telegram these were received: one past the last.
pub fn next_offset(updates: &[Update]) -> Option<i64> {
    updates.iter().map(|u| u.id + 1).max()
}

/// What to do with one update.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// Type this into the agent.
    Input(String),
    /// This chat is now the bridge's.
    Paired(i64),
    Ignore,
}

/// Which chat a bridge belongs to, and the code that claims it.
#[derive(Debug)]
pub struct Pairing {
    code: String,
    chat: Option<i64>,
}

impl Default for Pairing {
    fn default() -> Self {
        Self::new()
    }
}

impl Pairing {
    /// A bridge no chat has claimed yet, with a fresh code: twelve hex
    /// digits of a v4 uuid's random ones (48 bits), short enough to type.
    pub fn new() -> Self {
        Self { code: uuid::Uuid::new_v4().simple().to_string()[..12].to_string(), chat: None }
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn chat(&self) -> Option<i64> {
        self.chat
    }

    pub fn decide(&mut self, update: &Update) -> Decision {
        let (Some(chat), Some(text)) = (update.chat, update.text.as_deref()) else {
            return Decision::Ignore;
        };
        match self.chat {
            Some(paired) if paired == chat => Decision::Input(text.to_string()),
            Some(_) => Decision::Ignore,
            None if update.private && start_code(text) == Some(self.code.as_str()) => {
                self.chat = Some(chat);
                Decision::Paired(chat)
            }
            None => Decision::Ignore,
        }
    }
}

/// The code in `/start <code>`, as a `t.me/<bot>?start=<code>` link or a
/// typed command sends it (`/start@<bot> <code>` too).
fn start_code(text: &str) -> Option<&str> {
    let (command, code) = text.trim().split_once(char::is_whitespace)?;
    let command = command.split_once('@').map_or(command, |(c, _)| c);
    (command == "/start").then(|| code.trim())
}

/// The message for a turn's end: the agent's last reply when there is one.
pub fn turn_ended(label: &str, reply: Option<&str>) -> Vec<String> {
    match reply.map(str::trim).filter(|r| !r.is_empty()) {
        Some(reply) => chunks(&format!("{label}:\n\n{reply}")),
        None => chunks(&format!("{label} finished its turn.")),
    }
}

/// The message for a permission prompt the agent is waiting on: its tool
/// call when the transcript shows it, else what the hook said (Claude
/// Code's "Claude needs your permission to use Bash"), else that it asks.
pub fn asking(label: &str, asking: Option<&cmux_protocol::Asking>, message: Option<&str>) -> Vec<String> {
    let message = message.map(str::trim).filter(|m| !m.is_empty());
    match (asking, message) {
        (Some(a), _) if a.summary.trim().is_empty() => chunks(&format!("{label} is asking to use {}.", a.tool)),
        (Some(a), _) => chunks(&format!("{label} is asking to use {}:\n\n{}", a.tool, a.summary.trim())),
        (None, Some(message)) => chunks(&format!("{label}: {message}")),
        (None, None) => chunks(&format!("{label} is asking for permission.")),
    }
}

/// `text` in messages Telegram accepts: each at most [`MESSAGE_LIMIT`],
/// cut after the last line break that fits, or between two characters
/// when a line alone is too long.
pub fn chunks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let mut units = 0;
        let mut end = rest.len();
        let mut last_break = None;
        for (i, c) in rest.char_indices() {
            if units + c.len_utf16() > MESSAGE_LIMIT {
                end = i;
                break;
            }
            units += c.len_utf16();
            if c == '\n' {
                last_break = Some(i + 1);
            }
        }
        let cut = if end == rest.len() { end } else { last_break.unwrap_or(end) };
        out.push(rest[..cut].trim_end_matches('\n').to_string());
        rest = &rest[cut..];
    }
    // Telegram refuses a message with no text.
    out.retain(|c| !c.trim().is_empty());
    out
}

/// The last thing a Claude Code agent said in its last turn: the text of
/// its final reply in the transcript at `path`. None when that turn has no
/// text yet, or the transcript cannot be read.
pub fn last_reply(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut reply = LastReply::default();
    for line in std::io::BufReader::new(file).lines() {
        reply.feed(&line.ok()?);
    }
    reply.text
}

/// A transcript read for its last reply. Claude Code writes one reply as
/// several entries sharing its message id, one block each, so the text
/// blocks of one id are one reply; a new prompt starts the turn over.
#[derive(Debug, Default)]
struct LastReply {
    id: Option<String>,
    text: Option<String>,
}

impl LastReply {
    fn feed(&mut self, line: &str) {
        let Ok(entry) = serde_json::from_str::<Value>(line) else { return };
        if entry["isMeta"].as_bool() == Some(true) || entry["isSidechain"].as_bool() == Some(true) {
            return;
        }
        let content = &entry["message"]["content"];
        match entry["type"].as_str() {
            Some("user") => {
                let prompt = match content {
                    Value::String(_) => true,
                    Value::Array(blocks) => blocks.iter().any(|b| b["type"] == "text"),
                    _ => false,
                };
                if prompt {
                    *self = Self::default();
                }
            }
            Some("assistant") => {
                let said: Vec<&str> = content
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .filter(|t| !t.trim().is_empty())
                    .collect();
                if said.is_empty() {
                    return;
                }
                let id = text(&entry["message"]["id"]);
                let same = id.is_some() && id == self.id;
                let mut reply = if same { self.text.take().unwrap_or_default() } else { String::new() };
                for t in said {
                    if !reply.is_empty() {
                        reply.push_str("\n\n");
                    }
                    reply.push_str(t.trim());
                }
                self.id = id;
                self.text = Some(reply);
            }
            _ => {}
        }
    }
}

/// `message` with the bot token gone: an HTTP error names the URL it
/// failed on, and the token is in that URL
/// (`https://api.telegram.org/bot<token>/getUpdates`).
pub fn redact(message: &str, token: &str) -> String {
    if token.is_empty() {
        return message.to_string();
    }
    message.replace(token, "<token>")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(id: i64, chat: i64, private: bool, text: &str) -> Update {
        Update { id, chat: Some(chat), private, text: Some(text.into()) }
    }

    #[test]
    fn reads_updates_and_counts_past_what_is_not_text() {
        let body = r#"{"ok":true,"result":[
            {"update_id":10,"message":{"message_id":1,"chat":{"id":42,"type":"private"},"text":"hello"}},
            {"update_id":11,"message":{"message_id":2,"chat":{"id":42,"type":"private"},"photo":[{}]}},
            {"update_id":12,"edited_message":{"chat":{"id":42,"type":"private"},"text":"edited"}},
            {"update_id":13,"message":{"chat":{"id":-7,"type":"group"},"text":"hi all"}}
        ]}"#;
        let updates = parse_updates(body).unwrap();
        assert_eq!(updates[0], update(10, 42, true, "hello"));
        assert_eq!(updates[1], Update { id: 11, chat: Some(42), private: true, text: None });
        assert_eq!(updates[2], Update { id: 12, chat: None, private: false, text: None });
        assert_eq!(updates[3], update(13, -7, false, "hi all"));
        assert_eq!(next_offset(&updates), Some(14));
        assert_eq!(next_offset(&[]), None);
        assert_eq!(parse_updates(r#"{"ok":true,"result":[]}"#).unwrap(), vec![]);
    }

    #[test]
    fn a_refusal_is_telegrams_own_words() {
        let body = r#"{"ok":false,"error_code":401,"description":"Unauthorized"}"#;
        assert_eq!(parse_updates(body), Err("Unauthorized".into()));
        assert!(parse_updates("<html>").is_err());
        assert_eq!(answer(r#"{"ok":true,"result":{"username":"my_bot"}}"#).unwrap()["username"], "my_bot");
    }

    #[test]
    fn only_the_right_code_from_a_private_chat_pairs() {
        let mut pairing = Pairing::new();
        let code = pairing.code().to_string();
        assert_eq!(code.len(), 12);
        assert_ne!(code, Pairing::new().code(), "a fresh code each time");

        // A stranger who found the bot, a wrong code, a group.
        assert_eq!(pairing.decide(&update(1, 5, true, "rm -rf /")), Decision::Ignore);
        assert_eq!(pairing.decide(&update(2, 5, true, "/start")), Decision::Ignore);
        assert_eq!(pairing.decide(&update(3, 5, true, "/start 000000000000")), Decision::Ignore);
        assert_eq!(pairing.decide(&update(4, 5, true, &format!("/start {code}x"))), Decision::Ignore);
        assert_eq!(pairing.decide(&update(5, -9, false, &format!("/start {code}"))), Decision::Ignore);
        assert_eq!(pairing.chat(), None);

        assert_eq!(pairing.decide(&update(6, 42, true, &format!("/start {code}"))), Decision::Paired(42));
        assert_eq!(pairing.chat(), Some(42));
        assert_eq!(pairing.decide(&update(7, 42, true, "run the tests")), Decision::Input("run the tests".into()));

        // Once paired, the code is no way in for anyone else.
        assert_eq!(pairing.decide(&update(8, 5, true, &format!("/start {code}"))), Decision::Ignore);
        assert_eq!(pairing.decide(&update(9, 5, true, "hello")), Decision::Ignore);
        let photo = Update { id: 10, chat: Some(42), private: true, text: None };
        assert_eq!(pairing.decide(&photo), Decision::Ignore);
    }

    #[test]
    fn a_start_link_and_a_typed_start_both_carry_the_code() {
        assert_eq!(start_code("/start abc"), Some("abc"));
        assert_eq!(start_code("/start@my_bot abc "), Some("abc"));
        assert_eq!(start_code("/stop abc"), None);
        assert_eq!(start_code("/start"), None);
    }

    #[test]
    fn says_what_the_agent_replied_or_that_it_finished() {
        assert_eq!(turn_ended("planner", Some("All green.")), vec!["planner:\n\nAll green."]);
        assert_eq!(turn_ended("planner", Some("  \n")), vec!["planner finished its turn."]);
        assert_eq!(turn_ended("planner", None), vec!["planner finished its turn."]);
        let call = |summary: &str| cmux_protocol::Asking { tool: "Bash".into(), summary: summary.into() };
        let hook = Some("Claude needs your permission to use Bash");
        assert_eq!(asking("worker", Some(&call("cargo test")), hook), vec!["worker is asking to use Bash:\n\ncargo test"]);
        assert_eq!(asking("worker", Some(&call("")), None), vec!["worker is asking to use Bash."]);
        // No call found in the transcript: what the hook said, or that it asks.
        assert_eq!(asking("worker", None, hook), vec!["worker: Claude needs your permission to use Bash"]);
        assert_eq!(asking("worker", None, Some(" ")), vec!["worker is asking for permission."]);
        assert_eq!(asking("worker", None, None), vec!["worker is asking for permission."]);
    }

    #[test]
    fn splits_long_text_at_line_breaks_within_telegrams_limit() {
        let line = "x".repeat(3000);
        let text = format!("{line}\n{line}\n{line}");
        assert_eq!(chunks(&text), vec![line.clone(), line.clone(), line.clone()]);
        assert_eq!(chunks("short"), vec!["short"]);
        assert!(chunks("").is_empty());
        assert!(chunks(" \n\t\n").is_empty(), "nothing to send");
        // A run of blank lines at a cut leaves no blank message behind.
        let text = format!("{}\n{}\n{line}", "x".repeat(4095), " ".repeat(10));
        assert!(chunks(&text).iter().all(|c| !c.trim().is_empty()));
    }

    #[test]
    fn splits_multibyte_text_between_characters() {
        // No line break: cut between characters, counting as Telegram does.
        let text = "é".repeat(5000);
        let parts = chunks(&text);
        assert_eq!(parts.iter().map(|p| p.chars().count()).collect::<Vec<_>>(), vec![4096, 904]);
        assert_eq!(parts.concat(), text);

        // An emoji is two units to Telegram.
        let emoji = "🦀".repeat(3000);
        let parts = chunks(&emoji);
        assert!(parts.iter().all(|p| p.encode_utf16().count() <= MESSAGE_LIMIT));
        assert_eq!(parts[0].chars().count(), 2048);
        assert_eq!(parts.concat(), emoji);
    }

    fn reply_of(lines: &[&str]) -> Option<String> {
        let mut reply = LastReply::default();
        for line in lines {
            reply.feed(line);
        }
        reply.text
    }

    #[test]
    fn the_last_reply_is_the_final_text_of_the_last_turn() {
        let lines = [
            r#"{"type":"user","message":{"role":"user","content":"Fix the build"}}"#,
            r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"Looking."}]}}"#,
            r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{}}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#,
            r#"{"type":"assistant","message":{"id":"m2","content":[{"type":"thinking","thinking":"hmm"}]}}"#,
            r#"{"type":"assistant","message":{"id":"m2","content":[{"type":"text","text":"Fixed it."}]}}"#,
            r#"{"type":"assistant","message":{"id":"m2","content":[{"type":"text","text":"Tests pass."}],"stop_reason":"end_turn"}}"#,
            r#"{"type":"user","isMeta":true,"message":{"content":"<caveat>"}}"#,
            r#"{"type":"assistant","isSidechain":true,"message":{"id":"s","content":[{"type":"text","text":"a subagent"}]}}"#,
            "not json",
        ];
        assert_eq!(reply_of(&lines).as_deref(), Some("Fixed it.\n\nTests pass."));
        // A new prompt with no reply yet has none.
        let mut more = lines.to_vec();
        more.push(r#"{"type":"user","message":{"content":[{"type":"text","text":"Now ship it"}]}}"#);
        assert_eq!(reply_of(&more), None);
    }

    #[test]
    fn reads_the_last_reply_from_a_transcript_file() {
        let path = std::env::temp_dir().join(format!("mirador-telegram-{}.jsonl", uuid::Uuid::new_v4().simple()));
        std::fs::write(
            &path,
            "{\"type\":\"user\",\"message\":{\"content\":\"hi\"}}\n{\"type\":\"assistant\",\"message\":{\"id\":\"a\",\"content\":[{\"type\":\"text\",\"text\":\"Hello!\"}]}}\n",
        )
        .unwrap();
        assert_eq!(last_reply(&path).as_deref(), Some("Hello!"));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(last_reply(&path), None);
    }

    #[test]
    fn an_error_never_shows_the_token() {
        let token = "123456:ABC-def_GHI";
        let error = format!("error sending request for url (https://api.telegram.org/bot{token}/getUpdates?offset=3&timeout=50)");
        let redacted = redact(&error, token);
        assert!(!redacted.contains("ABC-def"), "{redacted}");
        assert_eq!(redacted, "error sending request for url (https://api.telegram.org/bot<token>/getUpdates?offset=3&timeout=50)");
        assert_eq!(redact("nothing secret", token), "nothing secret");
    }
}
