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
//! A name is looked up in the sender's tab — its teammates — by team
//! member name, then session name, then role. `<team>/<name>` looks it up
//! the same way in the tab running that team instead. A pane id reaches
//! any pane.

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
/// `team` is the running team whose tab the candidates are, when the name
/// came as `<team>/<name>`; none for the sender's own tab. It changes only
/// what a miss says.
pub fn resolve(name: &str, candidates: &[Candidate], team: Option<&str>) -> Result<String, String> {
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
    let known = if known.is_empty() { " — there are none".to_string() } else { format!(" (there are: {})", known.join(", ")) };
    Err(match team {
        Some(team) => format!("team `{team}` has no agent `{name}`{known}"),
        None => format!("no agent `{name}` in this tab{known}; `<team>/<name>` reaches another tab's team"),
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

/// How long to wait between what is typed and its Enter, so the Enter
/// comes on its own read and submits rather than joining the text.
///
/// Pi (1.3.0) and OpenCode (2.0.31) need none: in a PTY, each submitted a
/// bracketed paste as one message with the Enter in the same write, 0 ms
/// after it, or 120 ms after a 3000-character paste. They get the default.
///
/// Codex needs longer. Where it gets no bracketed paste (Windows' console
/// input gives it none), plain characters handled 8 ms apart or less
/// (`PASTE_BURST_CHAR_INTERVAL`) are a paste burst, and an Enter during
/// one, or within 120 ms of its last character
/// (`PASTE_ENTER_SUPPRESS_WINDOW`), is a newline in the input box. On
/// Windows a burst ends only after 60 ms with nothing typed
/// (`PASTE_BURST_ACTIVE_IDLE_TIMEOUT`), all in codex-rs
/// `tui/src/bottom_pane/paste_burst.rs`. Those times are when Codex
/// handles each key, not when it arrived, so for a long message the gap
/// must count from the burst's flush, not from the write: the app waits
/// for the flush on Codex's screen first, and 400 ms from there leaves
/// more than the whole 180 ms window again. `/new`, four plain characters
/// in one write, is short enough for 400 ms from the write.
pub fn enter_delay(harness: Option<cmux_protocol::Harness>) -> std::time::Duration {
    use std::time::Duration;
    match harness {
        Some(cmux_protocol::Harness::Codex) => Duration::from_millis(400),
        _ => Duration::from_millis(120),
    }
}

/// How long the app gives Codex to show a paste of `chars` characters
/// before its Enter goes anyway ([`PasteWatch`] says why it waits at all).
/// Measured on Windows with Codex 0.167.0, a paste shows about 0.5 ms per
/// character after the write (5075 characters took some 2.5 s); 2 ms is
/// four times that, for slower machines, on top of 5 s for the rest.
pub fn paste_wait(chars: usize) -> std::time::Duration {
    std::time::Duration::from_secs(5) + std::time::Duration::from_millis(2) * chars as u32
}

/// Whether a paste has landed in an agent's input box, by its screen: the
/// screen read before the paste, then each read after it.
///
/// Codex on Windows gets no bracketed paste, so each character is a key
/// event, and its paste-burst guard times them by when it handles them,
/// not when they arrived. A long message takes it longer than any fixed
/// gap to work through, and an Enter queued behind the text lands inside
/// the burst and becomes a newline: the message sits unsent. Codex redraws
/// its input box once the burst flushes — the text, or `[Pasted Content N
/// chars]` — so an input box that changed and then held still is that
/// flush, and [`enter_delay`] counts from there.
///
/// Only the input box is compared, from its line to the end of the screen.
/// A working Codex redraws a status line above it every second (a timer, a
/// spinner), and two reads of the whole screen could match each other, and
/// differ from the one before, while the burst is still being worked
/// through. A screen with no input box line is compared whole.
#[derive(Debug, Clone)]
pub struct PasteWatch {
    before: String,
    last: Option<String>,
}

impl PasteWatch {
    pub fn new(before: String) -> Self {
        PasteWatch { before: input_box(&before).to_string(), last: None }
    }

    /// One read of the screen: true once its input box differs from the one
    /// before the paste and is the same as on the read before this one.
    pub fn landed(&mut self, screen: &str) -> bool {
        let shown = input_box(screen);
        let still = self.last.as_deref() == Some(shown);
        self.last = Some(shown.to_string());
        still && shown != self.before
    }
}

/// `screen` from its last selection line on (see [`showing`]), or all of
/// it when it has none.
fn input_box(screen: &str) -> &str {
    selection_line(screen).map_or(screen, |at| &screen[at..])
}

/// Where `screen`'s last selection line starts: a glyph (Claude Code's
/// `❯`, Codex's `›`, or the ASCII `>` some of Codex's menus draw) alone or
/// before a space.
fn selection_line(screen: &str) -> Option<usize> {
    let mut found = None;
    let mut at = 0;
    for line in screen.split_inclusive('\n') {
        let mut chars = line.trim().chars();
        if matches!(chars.next(), Some('❯' | '›' | '>')) && chars.next().is_none_or(char::is_whitespace) {
            found = Some(at);
        }
        at += line.len();
    }
    found
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
        // Each starts a new session in the same process, keeping what it
        // was started with: Pi's `--append-system-prompt`, an OpenCode
        // role's agent (its prompt and model). Both seen on the requests
        // they sent next.
        Some(Harness::Pi | Harness::OpenCode) => Ok("/new"),
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

/// What `screen` shows, judged the way `harness` draws: Pi and OpenCode
/// by their own input boxes and menus ([`pi_showing`],
/// [`opencode_showing`]), any other by its selection glyph ([`showing`]).
pub fn showing_in(screen: &str, harness: Option<cmux_protocol::Harness>) -> Showing<'_> {
    match harness {
        Some(cmux_protocol::Harness::Pi) => pi_showing(screen),
        Some(cmux_protocol::Harness::OpenCode) => opencode_showing(screen),
        _ => showing(screen),
    }
}

/// Whether `screen` shows a Codex agent at work on a turn, which a
/// message typed now would steer (#246): Codex keeps its input box up
/// while it works, and Enter there goes into the running turn. What says
/// it is at work is its status line, the last one above the box: `•
/// Working (2s • esc to interrupt)`, its first words changing with what it
/// does — the parenthesis, which a bullet of its answer saying the same
/// words lacks. Any other harness: no. Claude Code takes a message typed
/// mid-turn at its next tool call.
pub fn at_work_on_screen(screen: &str, harness: Option<cmux_protocol::Harness>) -> bool {
    if harness != Some(cmux_protocol::Harness::Codex) {
        return false;
    }
    let Some(at) = selection_line(screen) else { return false };
    screen[..at]
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .is_some_and(|l| l.trim_start().starts_with('•') && l.contains("esc to interrupt)"))
}

/// Pi (1.3.0) has no selection glyph on its input box: the box is what is
/// between its last two rules (lines of `─`, the upper one reading `── ⠹
/// Working ──` while it works), with its footer — the directory, the
/// tokens and the model — below. Its menus are drawn between two rules too
/// (its project-trust question), their selected row marked `→ `, and with
/// nothing below the last rule; or in the box's own place, its pickers —
/// a model's, a setting's, a login's with `→ `, `/resume`'s sessions with
/// `› `. The commands listed while `/` is typed show under the box, not
/// in it, so they are no menu here.
fn pi_showing(screen: &str) -> Showing<'_> {
    let lines: Vec<&str> = screen.lines().collect();
    let is_rule = |l: &str| {
        let l = l.trim();
        l.starts_with("──") && l.ends_with('─')
    };
    let rules: Vec<usize> = (0..lines.len()).filter(|&i| is_rule(lines[i])).collect();
    let [.., top, bottom] = rules[..] else { return Showing::Neither };
    if let Some(row) = lines[top + 1..bottom].iter().map(|l| l.trim()).find(|l| l.starts_with("→ ") || l.starts_with("› ")) {
        return Showing::Choice(row);
    }
    if lines[bottom + 1..].iter().any(|l| !l.trim().is_empty()) {
        Showing::Composer
    } else {
        Showing::Neither
    }
}

/// OpenCode (2.0.31) draws its input box with a bar down its left (`┃`),
/// the agent and model on its last line (`┃  Build · Qwen Local`), and a
/// bottom edge of `╹▀▀▀…`. A permission it asks for takes the box's place:
/// `△ Permission required`, then `Allow once   Always allow   Reject` with
/// `⇆ select  enter confirm` beside it. Its other dialogs — the command
/// palette, `/models`, `/sessions`, `/themes` — are drawn over the box,
/// which still shows behind them, so they are looked for first: each has
/// a title row ending in `esc` (`Commands … esc`), which neither the box
/// nor a working one (`esc interrupt`, mid-line) has.
fn opencode_showing(screen: &str) -> Showing<'_> {
    if let Some(row) = screen.lines().map(str::trim).find(|l| l.ends_with(" esc")) {
        return Showing::Choice(row);
    }
    if let Some(row) = screen.lines().map(str::trim).find(|l| l.contains("enter confirm")) {
        return Showing::Choice(row);
    }
    if screen.lines().any(|l| l.trim().starts_with("╹▀")) {
        Showing::Composer
    } else {
        Showing::Neither
    }
}

/// What `screen` shows, judged by its last selection line
/// ([`selection_line`]). A menu row has a number, a dot and a space after
/// the glyph (`❯ 1. Yes`); any other is the input box.
pub fn showing(screen: &str) -> Showing<'_> {
    // The last such line, not any: while the input box is up its line
    // (`❯ `, `› Ask Codex…`) is the last, below past messages echoed as
    // `› 1. do X`; a dialog takes its place, and then its cursor is the
    // last.
    let Some(at) = selection_line(screen) else {
        return Showing::Neither;
    };
    let line = screen[at..].lines().next().unwrap_or_default().trim();
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

/// Whether the input box on `screen` holds something typed and not yet
/// sent: a person's half-written prompt that a pasted message would be
/// merged into. `cursor` is the terminal's cursor, as (row, column) from 0,
/// the row indexing `screen`'s lines; `after_cursor` is its row from the
/// cursor on, its dim cells blanked.
///
/// Judged by the cursor and dimness, not the text. An empty box is not
/// empty on screen: Claude Code draws a placeholder after its `❯ ` in a
/// fresh session (`Try "how does styles.css work?"`), Codex one after its
/// `› ` (`Ask Codex to do anything`), and both read as text. But both park
/// the cursor right after the glyph and its space while the box is empty,
/// and after what was typed once it is not — further along the glyph's
/// line, or on a line below it for a draft of several lines. A draft whose
/// cursor was moved back to its start (Home, Ctrl+A) parks it there too;
/// what tells it from the placeholder is that both harnesses draw the
/// placeholder dim (`ESC[2m`) and what was typed at normal intensity. So
/// with the cursor after the glyph, anything not dim after it is a draft.
///
/// Only Claude Code and Codex, whose glyph this finds the box by, are
/// judged; any other harness, and a screen that is not showing an input
/// box, has no draft here.
pub fn has_draft(screen: &str, cursor: (usize, usize), after_cursor: &str, harness: Option<cmux_protocol::Harness>) -> bool {
    use cmux_protocol::Harness;
    if !matches!(harness, Some(Harness::Claude | Harness::Codex)) || showing(screen) != Showing::Composer {
        return false;
    }
    let Some(at) = selection_line(screen) else { return false };
    let row = screen[..at].matches('\n').count();
    let line = screen[at..].lines().next().unwrap_or_default();
    let glyph = line.chars().take_while(|c| c.is_whitespace()).count();
    cursor != (row, glyph + 2) || !after_cursor.trim().is_empty()
}

/// How long after the person last typed into a pane a message waits
/// before it is typed there: one typed in between would land in their
/// draft, which they may be about to send.
pub const TYPING_QUIET: std::time::Duration = std::time::Duration::from_secs(2);

/// Why the person is in the way of something typed into their agent's
/// pane: a draft in its input box ([`has_draft`]), or keys typed within
/// [`TYPING_QUIET`]. `None` when neither.
pub fn person_busy(draft: bool, typed_recently: bool) -> Option<&'static str> {
    if draft {
        Some("the person has a draft in its pane")
    } else if typed_recently {
        Some("the person is typing in its pane")
    } else {
        None
    }
}

/// Why a message is queued for its pane rather than typed now, as `mira
/// agent send` says it: the person is busy there ([`person_busy`]), the
/// agent is `at_work` on a turn, which the message would land in, or
/// `ahead` messages are queued before it, which keeps their order. The
/// person's reason is the one given when both hold: theirs is what the
/// sender can do something about. `None` types it now.
pub fn queue_reason(draft: bool, typed_recently: bool, at_work: bool, ahead: usize) -> Option<String> {
    let behind = match ahead {
        0 => String::new(),
        1 => "; 1 message is ahead of it".to_string(),
        n => format!("; {n} messages are ahead of it"),
    };
    match person_busy(draft, typed_recently) {
        Some(why) if draft => Some(format!("{why}; it goes in once that is sent or cleared{behind}")),
        Some(why) => Some(format!("{why}; it goes in once they stop and its input box is empty{behind}")),
        None if at_work => Some(format!("it is at work; it goes in once its turn ends{behind}")),
        None if ahead > 0 => Some(behind.trim_start_matches("; ").to_string()),
        None => None,
    }
}

/// Whether `data`, written to a pane from its terminal, was typed by the
/// person — not only what the terminal sends of its own accord. xterm's
/// `onData` carries both: focus reports (`ESC[I`, `ESC[O`), mouse reports
/// (`ESC[<0;10;5M`, the older `ESC[M` and three bytes) and replies to a
/// program's queries (the cursor's position `ESC[5;1R`, device attributes
/// `ESC[?1;2c`, a mode's `ESC[?2004;1$y`, a window's size `ESC[8;24;80t`,
/// a status `ESC[0n`, OSC and DCS answers). Writes are coalesced, so one
/// may hold both; it is typing if anything but those is in it. An arrow or
/// a Backspace is typing: it edits a draft.
pub fn is_typing(data: &str) -> bool {
    let b = data.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != 0x1b {
            return true;
        }
        match b.get(i + 1) {
            Some(b'[') => {
                let start = i + 2;
                let mut end = start;
                while end < b.len() && (0x20..=0x3f).contains(&b[end]) {
                    end += 1;
                }
                let Some(&last) = b.get(end) else { return true };
                let params = &b[start..end];
                let report = match last {
                    b'I' | b'O' => params.is_empty(),
                    b'M' if params.is_empty() => {
                        // X10 mouse: three bytes of its own follow.
                        end += 3;
                        true
                    }
                    b'M' | b'm' => params.first() == Some(&b'<'),
                    b'R' | b't' | b'n' => true,
                    b'c' => matches!(params.first(), Some(b'?' | b'>')),
                    b'y' => params.ends_with(b"$"),
                    _ => false,
                };
                if !report {
                    return true;
                }
                i = end + 1;
            }
            Some(b']' | b'P') => {
                // Up to its BEL or its string terminator, ESC \.
                let rest = &b[i + 2..];
                let bel = rest.iter().position(|&c| c == 0x07).map(|at| at + 1);
                let st = rest.windows(2).position(|w| w == b"\x1b\\").map(|at| at + 2);
                let Some(len) = [bel, st].into_iter().flatten().min() else { return true };
                i += 2 + len;
            }
            // Alt with a key, SS3's arrows and F1–F4, a lone Escape.
            _ => return true,
        }
    }
    false
}

/// The person's keystrokes held back from panes a handoff is typing into.
/// A message is a paste and, a moment later, an Enter; a key the person
/// presses in between would land inside the message and be sent with it
/// (#244: a trailing `u` went out that way). So while a pane is held, what
/// the person types into it is kept, in order, and handed back once the
/// Enter is written, to be typed after it.
///
/// Holds nest: two handoffs typing into one pane at once hand the keys
/// back only when the last of them is done.
#[derive(Debug, Default)]
pub struct KeyHold {
    held: std::collections::HashMap<String, (usize, Vec<u8>)>,
}

impl KeyHold {
    /// Starts holding `pane`'s keystrokes.
    pub fn hold(&mut self, pane: &str) {
        self.held.entry(pane.to_string()).or_default().0 += 1;
    }

    /// Keeps `bytes` typed into `pane` if it is held, and says so; false
    /// means they are to be written now.
    pub fn keep(&mut self, pane: &str, bytes: &[u8]) -> bool {
        let Some((_, kept)) = self.held.get_mut(pane) else { return false };
        kept.extend_from_slice(bytes);
        true
    }

    /// Ends one hold on `pane`: what was kept, in the order typed, once no
    /// hold is left on it; nothing while another is.
    pub fn release(&mut self, pane: &str) -> Vec<u8> {
        let Some((holds, _)) = self.held.get_mut(pane) else { return Vec::new() };
        *holds -= 1;
        if *holds > 0 {
            return Vec::new();
        }
        self.held.remove(pane).map(|(_, kept)| kept).unwrap_or_default()
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
        assert_eq!(resolve("reviewer-2", &tab, None), Ok("p3".into()));
        assert_eq!(resolve(" planner ", &tab, None), Ok("p1".into()));
        assert_eq!(resolve("Fix the flaky test", &tab, None), Ok("p4".into()));
        let two = resolve("reviewer", &tab, None).unwrap_err();
        assert!(two.contains("more than one") && two.contains("reviewer-1, reviewer-2"), "{two}");
        let none = resolve("gate", &tab, None).unwrap_err();
        assert!(none.contains("no agent `gate`") && none.contains("planner"), "{none}");
        assert!(resolve("x", &[], None).unwrap_err().contains("there are none"));
        // A member's name wins over another agent's role of the same word.
        let mixed = [c("a", Some("dev"), None, Some("x")), c("b", None, None, Some("dev"))];
        assert_eq!(resolve("dev", &mixed, None), Ok("a".into()));
    }

    /// A miss in the sender's tab points at the form that reaches another
    /// tab's team; a miss in a named team's tab says which team.
    #[test]
    fn a_miss_says_where_it_looked() {
        let tab = [c("p1", Some("planner"), None, Some("planner"))];
        let here = resolve("lead", &tab, None).unwrap_err();
        assert_eq!(here, "no agent `lead` in this tab (there are: planner); `<team>/<name>` reaches another tab's team");
        let there = resolve("lead", &tab, Some("omakase")).unwrap_err();
        assert_eq!(there, "team `omakase` has no agent `lead` (there are: planner)");
        assert_eq!(resolve("lead", &[], Some("omakase")).unwrap_err(), "team `omakase` has no agent `lead` — there are none");
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
    fn codex_gets_its_enter_after_a_paste_burst_ends() {
        use cmux_protocol::Harness;
        use std::time::Duration;
        // Codex's Enter-suppress window plus Windows' burst idle timeout.
        assert!(enter_delay(Some(Harness::Codex)) > Duration::from_millis(120 + 60));
        for other in [Some(Harness::Claude), Some(Harness::Aider), None] {
            assert_eq!(enter_delay(other), Duration::from_millis(120));
        }
    }

    #[test]
    fn a_screen_that_never_changes_is_no_paste() {
        let mut watch = PasteWatch::new("› ".into());
        assert!((0..5).all(|_| !watch.landed("› ")));
    }

    #[test]
    fn a_changed_screen_lands_on_its_second_read() {
        let mut watch = PasteWatch::new("› ".into());
        assert!(!watch.landed("› [Pasted Content 1059 chars]"), "not on its first");
        assert!(watch.landed("› [Pasted Content 1059 chars]"));
    }

    #[test]
    fn a_screen_still_changing_is_waited_for() {
        let mut watch = PasteWatch::new("› ".into());
        for screen in ["› h", "› he", "› hel", "› hell", "› hello"] {
            assert!(!watch.landed(screen), "{screen}");
        }
    }

    #[test]
    fn a_screen_back_to_where_it_was_has_not_landed() {
        let mut watch = PasteWatch::new("› ".into());
        assert!(!watch.landed("› hello"));
        assert!(!watch.landed("› "));
        assert!(!watch.landed("› "), "still and unchanged is no paste");
    }

    #[test]
    fn a_status_line_ticking_above_the_input_box_is_no_paste() {
        let screen = |secs: u32| format!("• Working ({secs}s • esc to interrupt)\n\n› \n\n  100% context left\n");
        let mut watch = PasteWatch::new(screen(2));
        assert!((3..8).all(|secs| !watch.landed(&screen(secs))));
    }

    #[test]
    fn a_paste_lands_under_a_ticking_status_line() {
        let screen = |secs: u32, input: &str| format!("• Working ({secs}s • esc to interrupt)\n\n› {input}\n");
        let mut watch = PasteWatch::new(screen(2, ""));
        assert!(!watch.landed(&screen(3, "")));
        assert!(!watch.landed(&screen(4, "")));
        assert!(!watch.landed(&screen(5, "[Pasted Content 1700 chars]")), "not on its first");
        assert!(watch.landed(&screen(6, "[Pasted Content 1700 chars]")));
    }

    #[test]
    fn a_paste_waits_longer_the_longer_it_is() {
        use std::time::Duration;
        assert_eq!(paste_wait(0), Duration::from_secs(5));
        assert_eq!(paste_wait(5000), Duration::from_secs(15));
    }

    #[test]
    fn clears_only_a_harness_it_knows() {
        use cmux_protocol::Harness;
        assert_eq!(clear_line(Some(Harness::Claude)), Ok("/clear"));
        assert_eq!(clear_line(Some(Harness::Codex)), Ok("/new"));
        assert_eq!(clear_line(Some(Harness::Pi)), Ok("/new"));
        assert_eq!(clear_line(Some(Harness::OpenCode)), Ok("/new"));
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

    /// Codex 0.165 mid-turn: our team's real reviewer while it reviewed a
    /// commit, its screen read through `mira read-screen` by the planner
    /// (the last 20 lines). The input box stays up under "• Working".
    const CODEX_WORKING: &str = "\
  box screen was seen, a read cut short refuses
  rather than types; the welcome e2e asserts the
  refusal arrives in [10 s, 12 s). New REAL
  fixtures: Codex after a turn (your own pane),
  Claude while working (the planner's pane mid-
  turn). A Codex-mid-turn capture is being taken
  from your pane while you review this, and will
  be added as a fixture. APPROVE or CHANGES via
  mira agent send planner.


• I’ll inspect the timeout changes and the new
  real-screen fixtures, including whether an
  interrupted read after seeing a no-input state
  refuses safely.

• Working (2s • esc to interrupt)


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

    // Pi 1.3.0 and OpenCode 2.0.31, each driven in a PTY 110 columns wide
    // with a local model; blank runs and the directory shortened.
    const PI_IDLE: &str = "\
 I've received your request. Please provide the actual command or instruction you'd like me to process.
──────────────────────────────────────────────────────────────────────────────────────────────────────────────

──────────────────────────────────────────────────────────────────────────────────────────────────────────────
~/work/app
↑1.7k ↓223 1.5%/128k (auto)                                                                               qwen
";

    const PI_WORKING: &str = "\
 [message from planner, via mira agent send]
 Reply with only the word PELICAN.
 Second line: ignore it.
── ⠹ Working ─────────────────────────────────────────────────────────────────────────────────────────────────

──────────────────────────────────────────────────────────────────────────────────────────────────────────────
~/work/app
1.2%/128k (auto)                                                                                          qwen
";

    /// Pi with `/new` typed: the commands it matches listed under the box.
    const PI_TYPING_A_COMMAND: &str = "\
 Error: 500: {\"message\":\"recorder\"}
── ⠇ Retrying (2/3) in 2s... (escape to cancel) ──────────────────────────────────────────────────────────────
/new
──────────────────────────────────────────────────────────────────────────────────────────────────────────────
→ new         Start a new session
~/work/app
1.2%/128k (auto)                                                                                (rec) recmodel
";

    const PI_TRUST: &str = "\
 Pi can explain its own features and look up its docs. Ask it how to use or extend Pi.
──────────────────────────────────────────────────────────────────────────────────────────────────────────────

 Trust project folder?
 ~/work/app

 This allows pi to load .pi settings and resources, install missing project packages, and execute project
 extensions.

 → Trust
   Trust parent folder
 (~/work)
   Trust (this session only)
   Do not trust
   Do not trust (this session only)

 ↑↓ navigate  enter select  escape/ctrl+c cancel

──────────────────────────────────────────────────────────────────────────────────────────────────────────────
";

    const OPENCODE_IDLE: &str = "\
                                           /update to install v2.3.2

                  ┃
                  ┃  Ask anything… \"Fix broken tests\"
                  ┃
                  ┃  Build · Qwen Local
                  ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                  ~/work/app  shift+tab agents  ctrl+p commands
";

    const OPENCODE_WORKING: &str = "\
 ⠧ New session                   +
  ┃
  ┃  [message from planner, via mira agent send]
  ┃  Reply with only the word PELICAN.
  ┃  Second line: ignore it.
  ┃
  ┃  Build · Qwen Local
  ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
   ⬝⬝⬝⬝⬝⬝⬝⬝ esc interrupt                                                  shift+tab agents  ctrl+p commands
";

    const OPENCODE_PERMISSION: &str = "\
  ┃
  ┃  Use your bash tool to run the command: ls
  ┃

     + Thought · 1.4s

  ┃
  ┃  ⠼ ls
  ┃

  ┃
  ┃  △ Permission required
  ┃
  ┃  $ ls
  ┃
  ┃
  ┃   Allow once   Always allow   Reject                       ctrl+f fullscreen  ⇆ select  enter confirm
  ┃
";

    // Dialogs over or in place of the box, captured by the step's reviewer
    // on the same versions, trimmed the same way.
    const PI_RESUME: &str = "\
──────────────────────────────────────────────────────────────────────────────────────────────────────────────
Resume Session (Current Folder)                            ◉ Current Folder | ○ All  Name: All  Sort: Threaded
tab scope · re:<pattern> regex · \"phrase\" exact
ctrl+s sort · ctrl+n named · ctrl+d delete · ctrl+p path (off) · ctrl+r rename
>
› dev                                                                                                    3 43m
──────────────────────────────────────────────────────────────────────────────────────────────────────────────
~/work/app
0.0%/128k (auto)                                                                                          qwen
";

    const OPENCODE_PALETTE: &str = "\
                             Commands                                         esc
                             Search
                             Suggested
                             Switch session                              ctrl+x l
                             Switch model                                ctrl+x m
                             Connect an integration
                             Open settings
                  ┃
                  ┃  Ask     Session
                  ┃          Switch session                              ctrl+x l
                  ┃  Dev     New session                                 ctrl+x n
                  ╹▀▀▀▀▀▀    Clear session                                           ▀▀▀▀▀▀▀▀
";

    const OPENCODE_MODELS: &str = "\
                             Select model                                     esc
                             Search
                             Tidepool Alpha Free                             Free
                  ┃  Dev     Beam Free                                       Free
                  ╹▀▀▀▀▀▀    MiMo-V2.6-Flash Free                            Free    ▀▀▀▀▀▀▀▀
";

    const OPENCODE_SESSIONS: &str = "\
               Sessions for proj                                                            esc
               Search
               Today
               New session - 2026-11-19T13:40:41.609Z
               delete ctrl+d  rename ctrl+r                                 all projects ctrl+a
                  ┃
                  ┃  Ask anything… \"Fix a TODO in the codebase\"
                  ┃
                  ┃  Dev · qwen Local
                  ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
";

    const OPENCODE_THEMES: &str = "\
                             Themes                                           esc
                             Search
                  ┃
                  ┃  Dev · qwen Local
                  ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
";

    /// Pi and OpenCode, by what each draws: the input box, idle or
    /// working, is somewhere to type; Pi's trust question and OpenCode's
    /// permission are menus a message would answer; neither's banner or
    /// a shell is either.
    #[test]
    fn sees_pi_and_opencode_as_they_draw() {
        use cmux_protocol::Harness::{OpenCode, Pi};
        use Showing::{Choice, Composer, Neither};
        assert_eq!(showing_in(PI_IDLE, Some(Pi)), Composer);
        assert_eq!(showing_in(PI_WORKING, Some(Pi)), Composer);
        assert_eq!(showing_in(PI_TYPING_A_COMMAND, Some(Pi)), Composer);
        assert_eq!(showing_in(PI_TRUST, Some(Pi)), Choice("→ Trust"));
        assert_eq!(
            showing_in(PI_RESUME, Some(Pi)),
            Choice("› dev                                                                                                    3 43m")
        );
        assert_eq!(showing_in(OPENCODE_IDLE, Some(OpenCode)), Composer);
        assert_eq!(showing_in(OPENCODE_WORKING, Some(OpenCode)), Composer);
        assert_eq!(
            showing_in(OPENCODE_PERMISSION, Some(OpenCode)),
            Choice("┃   Allow once   Always allow   Reject                       ctrl+f fullscreen  ⇆ select  enter confirm")
        );
        // Its dialogs, though the box still shows behind them.
        for (screen, title) in [
            (OPENCODE_PALETTE, "Commands"),
            (OPENCODE_MODELS, "Select model"),
            (OPENCODE_SESSIONS, "Sessions for proj"),
            (OPENCODE_THEMES, "Themes"),
        ] {
            let Choice(row) = showing_in(screen, Some(OpenCode)) else { panic!("{title}: not a choice") };
            assert!(row.starts_with(title) && row.ends_with(" esc"), "{row}");
        }
        // OpenCode's banner before its box is drawn; Pi's header alone.
        let banner: String = OPENCODE_IDLE.lines().take(2).map(|l| format!("{l}\n")).collect();
        assert_eq!(showing_in(&banner, Some(OpenCode)), Neither);
        assert_eq!(showing_in(" ▀▀█  v1.3.0\n █▀ █ escape interrupt\n", Some(Pi)), Neither);
        let shell = "yoanmarti@mac app % pi\n";
        assert_eq!((showing_in(shell, Some(Pi)), showing_in(shell, Some(OpenCode))), (Neither, Neither));
        // Their screens are not judged by another's glyphs, nor theirs
        // by Claude Code's: `→ Trust` is no Claude Code menu.
        assert_eq!(showing_in(CLAUDE_DIALOG, Some(cmux_protocol::Harness::Claude)), Choice("❯ 1. Yes"));
        assert_eq!(showing(PI_TRUST), Neither);
    }

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
        assert_eq!(showing(CODEX_WORKING), Composer);
        assert_eq!(showing(CLAUDE_WORKING), Composer);
        // A number that is not a menu row's.
        assert_eq!(showing("› 2026 was a year\n"), Composer);
        assert_eq!(showing("› 1.5 seconds\n"), Composer);
    }

    /// Claude Code's input box, fresh: its placeholder after the glyph
    /// (as read-screen reads it: the dim text is text). The glyph is on
    /// row 1.
    const CLAUDE_FRESH: &str = "\
────────────────────────────────────────────────────────────────────────────────
❯ Try \"how does styles.css work?\"
────────────────────────────────────────────────────────────────────────────────
  ? for shortcuts
";

    #[test]
    fn sees_a_draft_by_where_the_cursor_is() {
        use cmux_protocol::Harness::{Claude, Codex};
        // Empty, placeholder or not: the cursor sits after `❯ `.
        assert!(!has_draft(CLAUDE_FRESH, (1, 2), "", Some(Claude)));
        assert!(!has_draft(CLAUDE_IDLE, (7, 2), "", Some(Claude)));
        // `pa` typed: the cursor moved past it.
        let typed = CLAUDE_IDLE.replace("❯ \n", "❯ pa\n");
        assert!(has_draft(&typed, (7, 4), "", Some(Claude)));
        // A draft of two lines: the cursor is on the line below the glyph.
        let two = CLAUDE_IDLE.replace("❯ \n", "❯ no, wait\n  and also\n");
        assert!(has_draft(&two, (8, 10), "", Some(Claude)));
        // Codex's placeholder, its glyph on row 10, and a draft there.
        assert!(!has_draft(CODEX_IDLE, (10, 2), "", Some(Codex)));
        let typed = CODEX_IDLE.replace("› Ask Codex to do anything", "› pause");
        assert!(has_draft(&typed, (10, 7), "", Some(Codex)));
    }

    #[test]
    fn sees_a_draft_with_the_cursor_moved_to_its_start() {
        use cmux_protocol::Harness::{Claude, Codex, Pi};
        // The cursor after the glyph, only the dim placeholder after it,
        // blanked: empty.
        assert!(!has_draft(CLAUDE_FRESH, (1, 2), "                              ", Some(Claude)));
        assert!(!has_draft(CODEX_IDLE, (10, 2), "                        ", Some(Codex)));
        // `pa` typed, then Home: the cursor is back after the glyph, but
        // what follows it is drawn at normal intensity.
        let typed = CLAUDE_IDLE.replace("❯ \n", "❯ pa\n");
        assert!(has_draft(&typed, (7, 2), "pa", Some(Claude)));
        let typed = CODEX_IDLE.replace("› Ask Codex to do anything", "› pa");
        assert!(has_draft(&typed, (10, 2), "pa   ", Some(Codex)));
        // Not a box: what follows the cursor does not make a draft.
        assert!(!has_draft(PI_IDLE, (0, 40), "pa", Some(Pi)));
    }

    #[test]
    fn finds_the_cursor_after_an_indented_glyph() {
        use cmux_protocol::Harness::Codex;
        let screen = "• ok\n\n  › Ask Codex to do anything\n";
        assert!(!has_draft(screen, (2, 4), "", Some(Codex)));
        assert!(has_draft(screen, (2, 2), "", Some(Codex)));
    }

    #[test]
    fn tells_typing_from_what_the_terminal_reports() {
        for typed in ["a", "pa", "\r", "\x7f", "\x1b[A", "\x1b[1;5C", "\x1b[3~", "\x1bOP", "\x1b", "\x1bb", "\x1b[200~hi\x1b[201~"] {
            assert!(is_typing(typed), "{typed:?} is typing");
        }
        for report in [
            "",
            "\x1b[I",
            "\x1b[O",
            "\x1b[<0;10;5M",
            "\x1b[<0;10;5m",
            "\x1b[M !!",
            "\x1b[5;1R",
            "\x1b[?1;2c",
            "\x1b[>0;276;0c",
            "\x1b[?2004;1$y",
            "\x1b[8;24;80t",
            "\x1b[0n",
            "\x1b]11;rgb:0000/0000/0000\x07",
            "\x1b]10;rgb:ffff/ffff/ffff\x1b\\",
            "\x1bP1$r0m\x1b\\",
        ] {
            assert!(!is_typing(report), "{report:?} is no typing");
        }
        // Coalesced: a focus report and a key in one write is typing; two
        // reports are not.
        assert!(is_typing("\x1b[Ix"));
        assert!(!is_typing("\x1b[I\x1b[<35;3;4M"));
        // Cut short, it may be anything: typing.
        assert!(is_typing("\x1b[<0;10"));
        assert!(is_typing("\x1b]11;rgb"));
    }

    #[test]
    fn queues_behind_a_person_or_another_message() {
        assert_eq!(queue_reason(false, false, false, 0), None);
        assert_eq!(
            queue_reason(true, true, true, 0).as_deref(),
            Some("the person has a draft in its pane; it goes in once that is sent or cleared")
        );
        assert_eq!(
            queue_reason(false, true, true, 2).as_deref(),
            Some("the person is typing in its pane; it goes in once they stop and its input box is empty; 2 messages are ahead of it")
        );
        assert_eq!(queue_reason(false, false, false, 1).as_deref(), Some("1 message is ahead of it"));
        // At work, no one in the way: its turn.
        assert_eq!(queue_reason(false, false, true, 0).as_deref(), Some("it is at work; it goes in once its turn ends"));
        assert_eq!(
            queue_reason(false, false, true, 1).as_deref(),
            Some("it is at work; it goes in once its turn ends; 1 message is ahead of it")
        );
        assert_eq!(person_busy(false, false), None);
        assert_eq!(person_busy(false, true), Some("the person is typing in its pane"));
    }

    #[test]
    fn sees_codex_at_work_by_its_status_line() {
        use cmux_protocol::Harness::{Claude, Codex};
        assert!(at_work_on_screen(CODEX_WORKING, Some(Codex)));
        // Its turn over, or never begun: the box alone.
        assert!(!at_work_on_screen(CODEX_AFTER_A_TURN, Some(Codex)));
        assert!(!at_work_on_screen(CODEX_IDLE, Some(Codex)));
        // At work with the person's text in its box, which reads the same.
        let typed = CODEX_WORKING.replace("› Ask Codex to do anything", "› pause");
        assert!(at_work_on_screen(&typed, Some(Codex)));
        // The words quoted higher up, in what it said: not its status line.
        let quoted = CODEX_IDLE.replace("• ok", "• Working (2s • esc to interrupt)\n\n• ok");
        assert!(!at_work_on_screen(&quoted, Some(Codex)));
        // An answer whose last bullet names the key: its turn is over.
        let answer = CODEX_IDLE.replace("• ok", "• Press esc to interrupt it.");
        assert!(!at_work_on_screen(&answer, Some(Codex)));
        // Codex only: Claude Code's hooks say when it works.
        assert!(!at_work_on_screen(CLAUDE_WORKING, Some(Claude)));
        assert!(!at_work_on_screen(CODEX_WORKING, Some(Claude)));
        assert!(!at_work_on_screen(CODEX_WORKING, None));
    }

    #[test]
    fn keys_pass_through_a_pane_not_held() {
        let mut hold = KeyHold::default();
        assert!(!hold.keep("p1", b"a"));
        hold.hold("p2");
        assert!(!hold.keep("p1", b"b"));
        assert_eq!(hold.release("p1"), b"");
    }

    #[test]
    fn held_keys_come_back_in_order_once_released() {
        let mut hold = KeyHold::default();
        hold.hold("p1");
        assert!(hold.keep("p1", b"pa"));
        assert!(hold.keep("p1", b"u"));
        assert_eq!(hold.release("p1"), b"pau");
        // Released, the pane takes keys again, and nothing is left.
        assert!(!hold.keep("p1", b"x"));
        assert_eq!(hold.release("p1"), b"");
    }

    #[test]
    fn a_nested_hold_keeps_keys_until_the_last_ends() {
        let mut hold = KeyHold::default();
        hold.hold("p1");
        hold.hold("p1");
        assert!(hold.keep("p1", b"a"));
        assert_eq!(hold.release("p1"), b"");
        assert!(hold.keep("p1", b"b"));
        assert_eq!(hold.release("p1"), b"ab");
    }

    #[test]
    fn sees_no_draft_where_it_cannot_judge() {
        use cmux_protocol::Harness::{Claude, Codex, OpenCode, Pi};
        // A menu is no input box, wherever the cursor is.
        assert!(!has_draft(CLAUDE_DIALOG, (11, 9), "", Some(Claude)));
        assert!(!has_draft(CODEX_APPROVAL, (0, 0), "", Some(Codex)));
        // Nor a screen with neither.
        assert!(!has_draft(CODEX_WELCOME, (0, 5), "", Some(Codex)));
        // Pi and OpenCode draw no glyph to place the cursor against.
        assert!(!has_draft(PI_IDLE, (0, 40), "", Some(Pi)));
        assert!(!has_draft(OPENCODE_IDLE, (0, 40), "", Some(OpenCode)));
        // Nor a harness that is not known.
        let typed = CLAUDE_IDLE.replace("❯ \n", "❯ pa\n");
        assert!(!has_draft(&typed, (7, 4), "", None));
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
