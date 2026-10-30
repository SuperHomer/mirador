//! Handoffs: one agent passing work to another through Mirador.
//!
//! `mira agent send <name> "…"` types a message into another agent's
//! terminal, as its next prompt; `mira agent wait <name>` blocks until that
//! agent has worked and stopped. A message is typed, so the rules here are
//! about what typing can do wrong:
//!
//! - into a plain shell it would run as a command: only a pane running an
//!   agent receives one;
//! - into a pane whose agent was started but is not running yet it would
//!   reach the shell first: the message waits for the agent, and is
//!   refused if it never comes up;
//! - into a permission prompt it would answer it: an agent waiting on a
//!   person is refused (unless forced);
//! - a newline would submit half of it: the message goes in as one
//!   bracketed paste, then Enter — and every escape and control character
//!   but newline and tab is taken out first, so it cannot end the paste
//!   early and type the rest as keystrokes.
//!
//! `mira agent clear` types an agent's own new-conversation command under
//! the first two rules; a working agent takes it once its turn is over.
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

/// Whether a pane's agent can be typed into yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    /// Started (launched, resumed, restored) but not yet seen running:
    /// its shell would take what is typed. Worth waiting for.
    Starting,
    /// Started longer ago than the app waits for any agent
    /// (`agents::START_GRACE`): it is not coming.
    NeverStarted,
    NotAgent,
}

/// Where a pane's agent is: `is_agent` and `live` (seen running) as the
/// pane says, `started` how long since it was started or resumed while
/// not yet seen. `sees_processes` is false on Windows, which lists no
/// processes, so nothing is ever seen running there: an agent counts as
/// ready, as it always did.
pub fn readiness(
    is_agent: bool,
    live: bool,
    started: Option<std::time::Duration>,
    sees_processes: bool,
) -> Readiness {
    if !is_agent {
        return Readiness::NotAgent;
    }
    match started {
        _ if live || !sees_processes => Readiness::Ready,
        Some(waited) if waited >= crate::agents::START_GRACE => Readiness::NeverStarted,
        Some(_) => Readiness::Starting,
        // Known from a hook, not started from here: nothing to wait for.
        None => Readiness::Ready,
    }
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

/// The line that starts `harness` on a new conversation, typed as plain
/// keystrokes: a slash command pasted is text in the prompt, not a command.
/// The process keeps running, so what it was launched with — a role's
/// prompt, Claude Code's `--append-system-prompt-file`, Codex's `-c
/// developer_instructions` — holds for the new conversation too.
pub fn clear_line(harness: Option<cmux_protocol::Harness>) -> Result<&'static str, String> {
    use cmux_protocol::Harness;
    match harness {
        Some(Harness::Claude) => Ok("/clear"),
        // Codex's own `/clear` also wipes the terminal; `/new` is the one
        // that only starts over.
        Some(Harness::Codex) => Ok("/new"),
        Some(other) => Err(format!("{} has no new-conversation command Mirador knows", other.label())),
        None => Err("its harness is not known".into()),
    }
}

/// What an agent's screen offers to type into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Showing<'a> {
    /// Its input box: what is typed goes in as a message.
    Composer,
    /// A menu, with this row selected: an Enter would pick it. Such a
    /// dialog — Codex's update prompt or an approval, Claude Code's own
    /// questions — sets no status a hook reports.
    Choice(&'a str),
    /// Neither: still starting, or a screen of its own (a logo, a
    /// welcome) that a keypress may turn into something else.
    Neither,
}

/// What `screen` shows, judged by its last selection line: a glyph
/// (Claude Code's `❯`, Codex's `›`, or the ASCII `>` some of Codex's
/// menus draw) alone or before a space. A menu row has a number, a dot
/// and a space after it (`❯ 1. Yes`); any other is the input box.
pub fn showing(screen: &str) -> Showing<'_> {
    // The last such line, not any: while the input box is up its line
    // (`❯ `, `› Ask Codex…`) is the last, below past messages echoed as
    // `› 1. do X`; a dialog takes its place, and then its cursor is the
    // last.
    let Some(line) = screen.lines().map(str::trim).rev().find(|l| {
        let mut chars = l.chars();
        matches!(chars.next(), Some('❯' | '›' | '>')) && chars.next().is_none_or(char::is_whitespace)
    }) else {
        return Showing::Neither;
    };
    let mut rest = line.chars();
    rest.next();
    let rest = rest.as_str().trim_start();
    let digits = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    if digits > 0 && rest[digits..].starts_with(". ") {
        Showing::Choice(line)
    } else {
        Showing::Composer
    }
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
    fn waits_for_an_agent_that_has_not_started_yet() {
        use std::time::Duration;
        use Readiness::*;
        let grace = crate::agents::START_GRACE;
        assert_eq!(readiness(false, false, None, true), NotAgent);
        assert_eq!(readiness(true, true, None, true), Ready);
        // Launched or restored, its shell still to type it in.
        assert_eq!(readiness(true, false, Some(Duration::from_millis(300)), true), Starting);
        assert_eq!(readiness(true, false, Some(grace), true), NeverStarted);
        // Seen once, then started again (`team restart`): live wins.
        assert_eq!(readiness(true, true, Some(Duration::ZERO), true), Ready);
        assert_eq!(readiness(true, false, None, true), Ready);
        // Windows sees no process: never waited for, as before.
        assert_eq!(readiness(true, false, Some(Duration::ZERO), false), Ready);
        assert_eq!(readiness(true, false, Some(grace), false), Ready);
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
    fn clears_only_a_harness_it_knows() {
        use cmux_protocol::Harness;
        assert_eq!(clear_line(Some(Harness::Claude)), Ok("/clear"));
        assert_eq!(clear_line(Some(Harness::Codex)), Ok("/new"));
        let aider = clear_line(Some(Harness::Aider)).unwrap_err();
        assert!(aider.contains("Aider"), "{aider}");
        assert!(clear_line(None).is_err());
    }

    /// Claude Code after a turn, asking (captured in a pane on this
    /// machine).
    const CLAUDE_DIALOG: &str = "\
⏺ ok

✻ Brewed for 2s · done 5:47 PM

────────────────────────────────────────────────────────────────────────────────
  Teach auto mode about your environment?

  Auto mode works better when it knows your environment. Takes about a
  minute.

  ❯ 1. Yes
";

    /// Claude Code idle: the same turn, then the composer between its
    /// rules. The past message above it, a numbered line itself, is added.
    const CLAUDE_IDLE: &str = "\
❯ 1. say ok

⏺ ok

✻ Brewed for 2s · done 5:47 PM

──────────────────────────────────────────────────────────────────────── lead ─
❯ 
────────────────────────────────────────────────────────────────────────────────
  ? for shortcuts
";

    /// Codex 0.165 signing in (rendered in a PTY with an empty CODEX_HOME):
    /// its cursor is an ASCII `>`.
    const CODEX_LOGIN: &str = "\
  Welcome to Codex, OpenAI's command-line coding agent

  Choose how you want to use Codex.

> 1. Sign in with ChatGPT
     Usage included with Plus, Pro, Business, and Enterprise plans
  2. Sign in with Device Code
     Sign in from another device with a one-time code
  3. Use an OpenAI API key
     Pay for what you use
  4. Use Amazon Bedrock
     Connect using your AWS credentials

  Press enter to continue
";

    /// Codex's update prompt. Reconstructed, not captured: the text is
    /// the strings in the Codex 0.165 binary, laid out as its menus are;
    /// this install only ever showed the banner without a menu.
    const CODEX_UPDATE: &str = "\
  ✨ Update available! 0.165.0 -> 0.166.0

  Release notes: https://github.com/openai/codex/releases/latest

› 1. Update now (runs `npm install -g @openai/codex`)
  2. Skip
  3. Skip until next version

  Press enter to continue
";

    /// Codex asking to run a command. Reconstructed, not captured, from
    /// the same binary's strings.
    const CODEX_APPROVAL: &str = "\
• Running cargo test

  Would you like to run the following command?

  $ cargo test

› 1. Yes, proceed (y)
  2. Yes, and don't ask again for commands that start with `cargo test` (p)
  3. No, and tell Codex what to do differently (esc)

  Press enter to confirm or esc to cancel
";

    /// Codex 0.165 idle, as rendered in a PTY, with a past message and
    /// its answer added above the composer.
    const CODEX_IDLE: &str = "\
╭──────────────────────────────────────────────────────────────╮
│ >_ OpenAI Codex (v0.165.0)                                   │
│                                                              │
│ directory: ~/proj                                            │
╰──────────────────────────────────────────────────────────────╯

› 1. do X

• ok

› Ask Codex to do anything

  ? for shortcuts
";

    /// Codex 0.165 starting with no login, its logo drawn and the menu
    /// below it not yet (rendered in a PTY; what a sandbox caught).
    const CODEX_WELCOME: &str = "\
                        ⣀⣀⣤⣤⣤⣤⣤⣄⣀⣀
                    ⢀⣤⣶⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣶⣦⣀
                  ⣠⣾⣿⣿⣿⣿⠿⠛⠛⠛⠛⠛⠛⠻⢿⣿⣿⣿⣿⣿⣷⣦
                ⣠⣾⣿⣿⣿⠟⠉           ⠙⠻⣿⣿⣿⣿⣷⣄
               ⢠⣿⣿⣿⡿⠁               ⠈⢿⣿⣿⣿⣿⣆
              ⢠⣿⣿⣿⡟⠁  ⢀⣤⣤⣄            ⠹⣿⣿⣿⣿⡄
              ⣾⣿⣿⣿⠁   ⢸⣿⣿⣿⣷⡀           ⢹⣿⣿⣿⣷
             ⢀⣿⣿⣿⡟     ⢻⣿⣿⣿⣷⡄           ⣿⣿⣿⣿⡄
             ⠘⣿⣿⣿⡇      ⣿⣿⣿⣿⣿           ⣿⣿⣿⣿⡇
              ⣿⣿⣿⣷     ⣼⣿⣿⣿⡿⠃ ⢀⣀⣀⣀⣀⣀⣀⡀  ⣿⣿⣿⣿⡇
              ⢽⣿⣿⣿⡆   ⢸⣿⣿⣿⡿⠁ ⢸⣿⣿⣿⣿⣿⣿⣿⣿⡆⢀⣿⣿⣿⣿⠃
              ⠘⢿⣿⣿⣿⡄   ⠉⠛⠋    ⠉⠛⠛⠛⠛⠛⠛⠛⠁⣸⣿⣿⣿⡿
               ⠘⣿⣿⣿⣿⣆                ⢀⣼⣿⣿⣿⣿⠃
                ⠘⢿⣿⣿⣿⣷⣄            ⢀⣤⣾⣿⣿⣿⡿⠃
                  ⠙⢿⣿⣿⣿⣿⣶⣤⣀⣀   ⣀⣀⣤⣶⣿⣿⣿⣿⣿⠟⠁
                    ⠉⠛⠿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡿⠛⠁
                        ⠉⠛⠻⠿⠿⠿⠿⠿⠿⠟⠛⠋⠁
";

    /// Codex 0.165 starting logged in (rendered in a PTY): its input box
    /// is up already, under a header whose `>_` is no glyph.
    const CODEX_LOADING: &str = "\
  >_ OpenAI Codex (v0.165.0)
     loading



                                             ⣀⣤⣤⣤⣀⣀
                                          ⣠⣶⣿⣿⣿⣿⣿⣿⣿⣿⣦⣤⣤⣤⣤⣤⣤⡀
                                        ⢀⣾⣿⣿⠿⠋⠉⠉⢉⣭⣿⣿⣿⣿⣿⠿⣿⣿⣿⣿⣷⣄
                                       ⣀⣾⣿⣿⠃ ⣀⣴⣾⣿⣿⡿⠟⠋ ⣀⡀ ⠈⠙⢿⣿⣿⣧
                                    ⣀⣶⣿⣿⣿⣿⡇ ⢸⣿⣿⡿⠛⠉⢀⣠⣴⣿⣿⣿⣷⣦⣀⠈⢻⣿⣿⡇
                                   ⣰⣿⣿⡿⢿⣿⣿⡇ ⢸⣿⣿⢀⣤⣶⣿⣿⣿⠟⠛⠻⣿⣿⣿⣿⣮⣿⣿⡷
                                  ⢰⣿⣿⡟⠁⢸⣿⣿⡇ ⢸⣿⣿⣿⣿⠿⠿⣿⣿⣿⣦⣄⡀⠈⠛⠿⣿⣿⣿⣧⡀
                                  ⣾⣿⣿⠃ ⢸⣿⣿⡇ ⢸⣿⣿⠋⠁  ⠈⠙⢻⣿⣿⣿⣶⣤⡀ ⠹⢿⣿⣷⡄
                                  ⢸⣿⣿⣇ ⠸⣿⣿⣷⣦⣼⣿⣿      ⢸⣿⣿⠻⢿⣿⣿⡇ ⠘⣿⣿⣿
                                  ⠈⢿⣿⣿⣦⡀⠈⠛⠿⣿⣿⣿⣿⣄⡀  ⢀⣠⣼⣿⣿  ⣿⣿⡇  ⣿⣿⣿⠇
                                    ⢻⣿⣿⣿⣷⣦⣀⠈⠙⠻⣿⣿⣿⣶⣶⣿⣿⣿⣿⣿  ⣿⣿⡇ ⣠⣿⣿⣿
                                    ⢸⣿⣿⡿⢿⣿⣿⣿⣦⣠⣴⣿⣿⣿⡿⠟⠋⢸⣿⣿  ⣿⣿⣷⣴⣿⣿⣿⠃
                                    ⠘⣿⣿⣷ ⠈⠛⢿⣿⣿⣿⠿⠋⠉ ⣠⣴⣿⣿⣿  ⣿⣿⣿⣿⣿⠟⠁
                                     ⠻⣿⣿⣿⣤⣀ ⠈⠉ ⢀⣤⣶⣿⣿⣿⠿⠟⠁ ⣰⣿⣿⡟⠋⠁
                                      ⠈⠿⣿⣿⣿⣿⣶⣾⣿⣿⣿⣿⠟⠋⠁  ⣠⣼⣿⣿⡟
                                         ⠙⠛⠛⠛⠻⠛⢿⣿⣿⣿⣿⣷⣾⣿⣿⣿⡿⠏
                                                ⠈⠙⠛⠿⠿⠿⠿⠛⠉



› Ask Codex to do anything


  ? for shortcuts
";

    /// Codex 0.165 idle after a turn: our team's real reviewer, its screen
    /// read through `mira read-screen` (the bottom of it).
    const CODEX_AFTER_A_TURN: &str = "\
• Explored · 1 failed
  └ Search read-screen-request|readScreen|
           serialize.*buffer|serialize\\( in src
    Read registry.ts
    + Show details

• The readiness checks run without holding app
  locks, but the advertised 10-second bound needs a
  closer look: each screen read can itself wait up
  to five seconds. I’m checking whether the retry
  loop can exceed that bound before I finalize.

• Explored
  └ Read server.rs, handoff.rs
    + Show details

• Ran /Applications/Mirador.app/Contents/MacOS/mir…
  └ e1ea323e-8fc7-409c-9bac-facfebc8bf64
    + Show details

• CHANGES sent to the planner: slow screen reads
  can push the refusal past the stated 10-second
  bound, and the Codex-while-working case still
  needs coverage.

  Worked for 50s • 12:53


› Ask Codex to do anything
";

    /// Claude Code mid-turn, a tool running: our team's real planner, its
    /// screen read through `mira read-screen` (the bottom of it). The input
    /// box stays up while it works.
    const CLAUDE_WORKING: &str = "\
⏺ I'm flagging two issues for the worker: the 10-second wait can stretch to nearly 15 seconds since
  each screen read takes up to 5 seconds and the deadline check happens after the read, and there's no
  test screen for Codex mid-turn, which risks wrongly blocking a send that would otherwise work.

  Sending the reviewer's two changes to the worker · 29s
  ⎿  $ /Applications/Mirador.app/Contents/MacOS/mira agent send worker --wait --timeout 590 \"Reviewer
     CHANGES on ad2b281 (step 3), two things:
     1) The 10 s bound isn't one: screen_refusal checks the deadline only after each read_screen,
     which can itself block up to 5 s, so a read started just before 10 s… (27s)
     (ctrl+b to run in background)

✳ Fluttering… (1m 53s · ↓ 3.7k tokens)
                                                              ✔ Update installed · /restart to apply
──────────────────────────────────────────────────────────────────────────────────────────── planner ─
❯
";

    #[test]
    fn sees_a_menu_the_agent_is_showing() {
        use Showing::Choice;
        assert_eq!(showing(CLAUDE_DIALOG), Choice("❯ 1. Yes"));
        assert_eq!(showing(CODEX_LOGIN), Choice("> 1. Sign in with ChatGPT"));
        assert_eq!(showing(CODEX_UPDATE), Choice("› 1. Update now (runs `npm install -g @openai/codex`)"));
        assert_eq!(showing(CODEX_APPROVAL), Choice("› 1. Yes, proceed (y)"));
        // Moved down, the cursor is still the one selection.
        let second = CODEX_UPDATE.replace("› 1. Update", "  1. Update").replace("  2. Skip\n", "› 2. Skip\n");
        assert_eq!(showing(&second), Choice("› 2. Skip"));
    }

    #[test]
    fn sees_the_input_box_however_it_reads() {
        use Showing::Composer;
        assert_eq!(showing(CLAUDE_IDLE), Composer);
        assert_eq!(showing(&CLAUDE_IDLE.replace("❯ \n", "❯ fix the bug\n")), Composer);
        assert_eq!(showing(CODEX_IDLE), Composer);
        assert_eq!(showing(&CODEX_IDLE.replace("› Ask Codex to do anything", "› fix the bug")), Composer);
        assert_eq!(showing(CODEX_LOADING), Composer);
        assert_eq!(showing(CODEX_AFTER_A_TURN), Composer);
        assert_eq!(showing(CLAUDE_WORKING), Composer);
        // A number that is not a menu row's.
        assert_eq!(showing("› 2026 was a year\n"), Composer);
        assert_eq!(showing("› 1.5 seconds\n"), Composer);
    }

    #[test]
    fn sees_neither_on_a_screen_with_no_glyph() {
        use Showing::Neither;
        assert_eq!(showing(CODEX_WELCOME), Neither);
        assert_eq!(showing(""), Neither);
        // Its loading header alone, its input box not drawn: `>_` is no glyph.
        let header: String = CODEX_LOADING.lines().take(2).map(|l| format!("{l}\n")).collect();
        assert!(header.contains(">_ OpenAI Codex"), "{header}");
        assert_eq!(showing(&header), Neither);
        assert_eq!(showing("yoanmarti@mac mirador % ls\nREADME.md  src\nyoanmarti@mac mirador % "), Neither);
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
