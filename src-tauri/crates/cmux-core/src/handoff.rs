//! Handoffs: one agent passing work to another through Mirador.
//!
//! `mira agent send <name> "…"` types a message into another agent's
//! terminal, as its next prompt; `mira agent wait <name>` blocks until that
//! agent has worked and stopped. A message is typed, so the rules here are
//! about what typing can do wrong:
//!
//! - into a plain shell it would run as a command: only a pane running an
//!   agent receives one;
//! - into a permission prompt it would answer it: an agent waiting on a
//!   person is refused (unless forced);
//! - a newline would submit half of it: the message goes in as one
//!   bracketed paste, then Enter — and every escape and control character
//!   but newline and tab is taken out first, so it cannot end the paste
//!   early and type the rest as keystrokes.
//!
//! A name is looked up in the sender's tab only — its teammates — by
//! team member name, then session name, then role. A pane id reaches any
//! pane.

use cmux_protocol::AgentStatus;

/// An agent a name may refer to: a pane of the sender's tab.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Candidate {
    pub pane: String,
    /// Its name in the tab's team, if it is a member.
    pub member: Option<String>,
    /// Its Claude Code session's name.
    pub session: Option<String>,
    pub role: Option<String>,
}

/// The pane `name` refers to among `candidates`: a team member's name
/// first, then a session's, then a role's — the first kind that matches
/// at all, which must match one pane. Two reviewers are `reviewer-1` and
/// `reviewer-2`, never "reviewer".
pub fn resolve(name: &str, candidates: &[Candidate]) -> Result<String, String> {
    let name = name.trim();
    let kinds: [fn(&Candidate) -> Option<&String>; 3] = [|c| c.member.as_ref(), |c| c.session.as_ref(), |c| c.role.as_ref()];
    for kind in kinds {
        let hits: Vec<&Candidate> = candidates.iter().filter(|c| kind(c).is_some_and(|n| n == name)).collect();
        match hits.as_slice() {
            [] => continue,
            [one] => return Ok(one.pane.clone()),
            many => {
                let names: Vec<String> = many.iter().map(|c| label(c)).collect();
                return Err(format!("`{name}` is more than one agent here: {}", names.join(", ")));
            }
        }
    }
    let known: Vec<String> = candidates.iter().map(label).collect();
    Err(if known.is_empty() {
        format!("no agent `{name}` in this tab — there are none")
    } else {
        format!("no agent `{name}` in this tab (there are: {})", known.join(", "))
    })
}

fn label(c: &Candidate) -> String {
    c.member
        .clone()
        .or_else(|| c.session.clone())
        .or_else(|| c.role.clone())
        .unwrap_or_else(|| c.pane.clone())
}

/// What a sender may call itself in a message: the same names, in the same
/// order.
pub fn sender_name(c: &Candidate) -> Option<String> {
    c.member.clone().or_else(|| c.session.clone()).or_else(|| c.role.clone())
}

/// Whether an agent with `status` may be typed into. One waiting on a
/// person is in a prompt the message would answer.
pub fn can_receive(status: Option<AgentStatus>, force: bool) -> Result<(), String> {
    if status == Some(AgentStatus::NeedsYou) && !force {
        return Err("it is waiting on a person (a permission prompt, a question): \
                    a message would answer it. Try again once it moves on, or --force"
            .into());
    }
    Ok(())
}

/// The bytes to write to the recipient's terminal: the message, said to be
/// from `from`, as one bracketed paste, then Enter as its own write — a TUI
/// that sees the paste and the Enter in one read can take the Enter as
/// part of the paste.
pub fn keystrokes(message: &str, from: Option<&str>) -> Result<(Vec<u8>, Vec<u8>), String> {
    let clean: String = message
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect::<String>()
        .replace('\t', "  ");
    let clean = clean.trim();
    if clean.is_empty() {
        return Err("the message is empty".into());
    }
    let body = match from {
        Some(from) => format!("[message from {from}, via mira agent send]\n{clean}"),
        None => clean.to_string(),
    };
    let mut paste = b"\x1b[200~".to_vec();
    paste.extend_from_slice(body.as_bytes());
    paste.extend_from_slice(b"\x1b[201~");
    Ok((paste, b"\r".to_vec()))
}

/// What `mira agent wait` has seen so far, and whether it is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Wait {
    pub started_ms: u64,
    pub seen_working: bool,
}

/// How a wait ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitEnd {
    /// It worked and stopped: its turn is over.
    Idle,
    /// It stopped to ask a person something.
    NeedsYou,
    /// Its agent is gone: the pane is a shell again, or closed.
    Exited,
}

impl Wait {
    /// One look at the agent: `None` while it is gone (`present` false),
    /// else its status and when that last changed. Done once the agent has
    /// stopped *since the wait began* — seen working, or changed status
    /// after it — so a wait started just after a `send` does not end on the
    /// idle it had before the message arrived.
    pub fn observe(&mut self, present: bool, status: Option<AgentStatus>, since_ms: Option<u64>) -> Option<WaitEnd> {
        if !present {
            return Some(WaitEnd::Exited);
        }
        let changed_since = since_ms.is_some_and(|s| s >= self.started_ms);
        match status {
            Some(AgentStatus::Working) => {
                self.seen_working = true;
                None
            }
            Some(AgentStatus::Idle) if self.seen_working || changed_since => Some(WaitEnd::Idle),
            Some(AgentStatus::NeedsYou) if self.seen_working || changed_since => Some(WaitEnd::NeedsYou),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(pane: &str, member: Option<&str>, session: Option<&str>, role: Option<&str>) -> Candidate {
        Candidate {
            pane: pane.into(),
            member: member.map(String::from),
            session: session.map(String::from),
            role: role.map(String::from),
        }
    }

    #[test]
    fn finds_a_teammate_by_name() {
        let tab = [
            c("p1", Some("planner"), Some("planner"), Some("planner")),
            c("p2", Some("reviewer-1"), Some("reviewer-1"), Some("reviewer")),
            c("p3", Some("reviewer-2"), Some("reviewer-2"), Some("reviewer")),
            c("p4", None, Some("Fix the flaky test"), None),
        ];
        assert_eq!(resolve("reviewer-2", &tab), Ok("p3".into()));
        assert_eq!(resolve(" planner ", &tab), Ok("p1".into()));
        assert_eq!(resolve("Fix the flaky test", &tab), Ok("p4".into()));
        let two = resolve("reviewer", &tab).unwrap_err();
        assert!(two.contains("more than one") && two.contains("reviewer-1, reviewer-2"), "{two}");
        let none = resolve("gate", &tab).unwrap_err();
        assert!(none.contains("no agent `gate`") && none.contains("planner"), "{none}");
        assert!(resolve("x", &[]).unwrap_err().contains("there are none"));
        // A member's name wins over another agent's role of the same word.
        let mixed = [c("a", Some("dev"), None, Some("x")), c("b", None, None, Some("dev"))];
        assert_eq!(resolve("dev", &mixed), Ok("a".into()));
    }

    #[test]
    fn refuses_an_agent_waiting_on_a_person() {
        assert!(can_receive(Some(AgentStatus::NeedsYou), false).is_err());
        assert!(can_receive(Some(AgentStatus::NeedsYou), true).is_ok());
        assert!(can_receive(Some(AgentStatus::Working), false).is_ok());
        assert!(can_receive(None, false).is_ok());
    }

    #[test]
    fn types_the_message_as_one_paste() {
        let (paste, enter) = keystrokes("Tests pass.\nPlease review\tsrc/auth.rs", Some("dev-1")).unwrap();
        assert_eq!(
            String::from_utf8(paste).unwrap(),
            "\x1b[200~[message from dev-1, via mira agent send]\nTests pass.\nPlease review  src/auth.rs\x1b[201~"
        );
        assert_eq!(enter, b"\r");
        // An escape cannot end the paste early and type the rest.
        let (evil, _) = keystrokes("hi\x1b[201~\rrm -rf ~\x03", None).unwrap();
        let evil = String::from_utf8(evil).unwrap();
        assert_eq!(evil, "\x1b[200~hi[201~rm -rf ~\x1b[201~");
        assert_eq!(evil.matches('\x1b').count(), 2);
        assert!(keystrokes(" \n\x07 ", None).is_err());
    }

    #[test]
    fn a_wait_ends_on_a_stop_after_it_began() {
        use AgentStatus::*;
        // Idle from before the message: keep waiting.
        let mut w = Wait { started_ms: 1000, seen_working: false };
        assert_eq!(w.observe(true, Some(Idle), Some(500)), None);
        assert_eq!(w.observe(true, Some(Working), Some(1100)), None);
        assert_eq!(w.observe(true, Some(Idle), Some(1200)), Some(WaitEnd::Idle));
        // Working and idle again between two looks: the change is enough.
        let mut quick = Wait { started_ms: 1000, seen_working: false };
        assert_eq!(quick.observe(true, Some(Idle), Some(1050)), Some(WaitEnd::Idle));
        let mut asked = Wait { started_ms: 1000, seen_working: true };
        assert_eq!(asked.observe(true, Some(NeedsYou), Some(900)), Some(WaitEnd::NeedsYou));
        assert_eq!(Wait::default().observe(false, None, None), Some(WaitEnd::Exited));
        // No status at all (no hooks): only its exit ends the wait.
        assert_eq!(Wait::default().observe(true, None, None), None);
    }
}
