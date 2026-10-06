//! What a holder sends a client that attaches: the last few megabytes of
//! output, prefixed with the terminal modes that were in force where those
//! megabytes begin.
//!
//! Raw replay rather than a terminal emulator (see
//! docs/design/session-persistence.md): a fresh xterm fed these bytes ends
//! up in the same modes and close to the same screen, and full-screen
//! programs redraw on the resize that follows an attach. Two things make
//! raw bytes go wrong, and both are handled here:
//!
//! - **Starting mid-sequence.** The buffer is only ever trimmed at a line
//!   feed or an ESC, never inside an escape sequence or a UTF-8 character.
//! - **Modes set before the window.** A program that entered the alternate
//!   screen 4 MB ago would replay onto the wrong screen. The modes that
//!   matter are tracked over the bytes as they are trimmed away, and
//!   re-asserted at the front.
//! - **Questions already answered.** Programs ask the terminal things —
//!   where is the cursor, what are you, what colour is the background —
//!   and the terminal answers by typing into their input. Replayed, the
//!   question is asked again and xterm answers again, so a shell at its
//!   prompt receives `^[[1;1R` as if typed. Windows' ConPTY asks for the
//!   cursor at the start of every session, so there it would be every
//!   reattach. Queries are removed from the replay; they were answered the
//!   first time.

use std::collections::{BTreeMap, VecDeque};

/// The DEC private modes worth carrying across a trim: which screen a
/// program is drawing on, how the keyboard and mouse talk to it, and
/// whether the cursor is shown. Everything else is either redrawn by the
/// program or harmless to lose.
const TRACKED: &[u16] = &[
    1049, 1047, 47, // alternate screen
    1,    // application cursor keys
    25,   // cursor visible
    1000, 1002, 1003, 1005, 1006, 1015, // mouse reporting and encodings
    2004, // bracketed paste
];

/// Watches a byte stream for `CSI ? Pn ; … h|l` and records the last value
/// of each tracked mode. Stateful across chunks: a sequence split between
/// two writes is still seen.
#[derive(Debug, Default, Clone)]
pub struct ModeTracker {
    /// Only modes the stream has set or reset; absent means "never touched",
    /// which a fresh terminal already agrees with.
    modes: BTreeMap<u16, bool>,
    state: State,
}

#[derive(Debug, Default, Clone)]
enum State {
    #[default]
    Ground,
    Esc,
    /// Inside `CSI`; `private` once a `?` has been seen, `params` collected.
    Csi {
        private: bool,
        params: Vec<u16>,
        current: Option<u16>,
    },
}

impl ModeTracker {
    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.step(b);
        }
    }

    fn step(&mut self, b: u8) {
        self.state = match std::mem::take(&mut self.state) {
            State::Ground | State::Esc if b == 0x1b => State::Esc,
            State::Ground => State::Ground,
            State::Esc if b == b'[' => State::Csi {
                private: false,
                params: Vec::new(),
                current: None,
            },
            State::Esc => State::Ground,
            State::Csi {
                private,
                mut params,
                current,
            } => match b {
                b'?' if params.is_empty() && current.is_none() => State::Csi {
                    private: true,
                    params,
                    current,
                },
                b'0'..=b'9' => State::Csi {
                    private,
                    params,
                    current: Some(
                        current
                            .unwrap_or(0)
                            .saturating_mul(10)
                            .saturating_add((b - b'0') as u16),
                    ),
                },
                b';' => {
                    params.push(current.unwrap_or(0));
                    State::Csi {
                        private,
                        params,
                        current: None,
                    }
                }
                // A new ESC aborts the sequence and starts another.
                0x1b => State::Esc,
                // Final byte: the sequence is complete.
                0x40..=0x7e => {
                    if let Some(c) = current {
                        params.push(c);
                    }
                    if private && (b == b'h' || b == b'l') {
                        for p in params {
                            if TRACKED.contains(&p) {
                                self.modes.insert(p, b == b'h');
                            }
                        }
                    }
                    State::Ground
                }
                // Intermediates and anything unexpected: keep reading until
                // the final byte rather than guess.
                _ => State::Csi {
                    private,
                    params,
                    current,
                },
            },
        };
    }

    /// Escape sequences that put a fresh terminal into these modes. The
    /// alternate screen goes first, so the rest apply to the screen the
    /// replayed bytes will draw on.
    pub fn as_sequences(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut emit = |mode: u16, on: bool| {
            out.extend_from_slice(format!("\x1b[?{mode}{}", if on { 'h' } else { 'l' }).as_bytes());
        };
        for screen in [1049, 1047, 47] {
            if let Some(&on) = self.modes.get(&screen) {
                emit(screen, on);
            }
        }
        for (&mode, &on) in &self.modes {
            if ![1049, 1047, 47].contains(&mode) {
                emit(mode, on);
            }
        }
        out
    }
}

/// How far past the capacity a trim may look for a safe place to cut
/// before giving up and cutting anyway.
const SAFE_CUT_WINDOW: usize = 4096;

/// The last `capacity` bytes of a pane's output, plus the modes in force at
/// the first of them.
pub struct ReplayBuffer {
    capacity: usize,
    bytes: VecDeque<u8>,
    /// Modes after everything trimmed off the front: the state the first
    /// kept byte was written in.
    head: ModeTracker,
}

impl ReplayBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            bytes: VecDeque::new(),
            head: ModeTracker::default(),
        }
    }

    pub fn push(&mut self, data: &[u8]) {
        self.bytes.extend(data);
        if self.bytes.len() <= self.capacity {
            return;
        }
        let mut cut = self.bytes.len() - self.capacity;
        // Move the cut forward to just past a line feed, or onto an ESC —
        // both ASCII, so never inside a UTF-8 character, and neither occurs
        // inside a CSI sequence.
        // A cut that already falls at the start of a line stays put.
        let at_line_start = self.bytes[cut - 1] == b'\n';
        if !at_line_start {
            let limit = (cut + SAFE_CUT_WINDOW).min(self.bytes.len());
            if let Some(safe) = (cut..limit).find(|&i| matches!(self.bytes[i], b'\n' | 0x1b)) {
                cut = if self.bytes[safe] == b'\n' { safe + 1 } else { safe };
            }
        }
        let dropped: Vec<u8> = self.bytes.drain(..cut).collect();
        self.head.feed(&dropped);
    }

    /// The modes at the front, then the kept bytes.
    pub fn snapshot(&self) -> Vec<u8> {
        let mut out = self.head.as_sequences();
        let kept: Vec<u8> = self.bytes.iter().copied().collect();
        out.extend(strip_queries(&kept));
        out
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

/// Removes the sequences a program sends to *ask* the terminal something,
/// leaving everything that draws or sets state. An unterminated sequence at
/// the end is kept as is: it may be the start of something still arriving.
pub fn strip_queries(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] != 0x1b || i + 1 >= input.len() {
            out.push(input[i]);
            i += 1;
            continue;
        }
        let end = match input[i + 1] {
            b'[' => csi_end(input, i + 2),
            b']' | b'P' => string_end(input, i + 2),
            _ => None,
        };
        let Some(end) = end else {
            out.push(input[i]);
            i += 1;
            continue;
        };
        let seq = &input[i..end];
        if !is_query(seq) {
            out.extend_from_slice(seq);
        }
        i = end;
    }
    out
}

/// One past a CSI sequence's final byte, if it is complete.
fn csi_end(input: &[u8], from: usize) -> Option<usize> {
    input[from..]
        .iter()
        .position(|b| (0x40..=0x7e).contains(b))
        .map(|p| from + p + 1)
}

/// One past an OSC/DCS string's terminator (BEL or ESC \), if complete.
fn string_end(input: &[u8], from: usize) -> Option<usize> {
    let mut j = from;
    while j < input.len() {
        match input[j] {
            0x07 => return Some(j + 1),
            0x1b if input.get(j + 1) == Some(&b'\\') => return Some(j + 2),
            _ => j += 1,
        }
    }
    None
}

fn is_query(seq: &[u8]) -> bool {
    match seq[1] {
        b'[' => {
            let body = &seq[2..seq.len() - 1];
            let fin = seq[seq.len() - 1];
            let params: Vec<u8> = body.iter().copied().filter(|b| (0x30..=0x3f).contains(b)).collect();
            let inter: Vec<u8> = body.iter().copied().filter(|b| (0x20..=0x2f).contains(b)).collect();
            let first: u32 = params
                .iter()
                .skip_while(|b| !b.is_ascii_digit())
                .take_while(|b| b.is_ascii_digit())
                .fold(0, |n, b| n.saturating_mul(10).saturating_add((b - b'0') as u32));
            match fin {
                // Device status report: cursor position, ok-ness, ...
                b'n' => true,
                // Device attributes; `CSI ? ... c` is a terminal's *reply*.
                b'c' => !params.starts_with(b"?"),
                // XTVERSION.
                b'q' => params.starts_with(b">"),
                // Kitty keyboard protocol query (push and pop are `>u`, `<u`).
                b'u' => params.starts_with(b"?"),
                // DECRQM: is this mode set?
                b'p' => inter.contains(&b'$'),
                // Window reports: size in pixels or cells, position, title.
                b't' => matches!(first, 11 | 13 | 14 | 15 | 16 | 18 | 19 | 20 | 21),
                _ => false,
            }
        }
        b']' => {
            // OSC: a query's last field is `?` — colours (4, 10, 11, 12 ...)
            // and, importantly, a clipboard read (52).
            let body = &seq[2..];
            let body = body
                .strip_suffix(b"\x07")
                .or_else(|| body.strip_suffix(b"\x1b\\"))
                .unwrap_or(body);
            body.ends_with(b";?")
        }
        // DCS: DECRQSS (`$q`) and XTGETTCAP (`+q`) ask; the rest set.
        b'P' => seq[2..].starts_with(b"$q") || seq[2..].starts_with(b"+q"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modes(stream: &[u8]) -> Vec<u8> {
        let mut t = ModeTracker::default();
        t.feed(stream);
        t.as_sequences()
    }

    #[test]
    fn tracks_private_modes_and_ignores_the_rest() {
        assert_eq!(modes(b"\x1b[?1049h\x1b[?2004h\x1b[31mhi"), b"\x1b[?1049h\x1b[?2004h");
        // Several modes in one sequence; a later reset wins.
        assert_eq!(modes(b"\x1b[?1000;1006h\x1b[?1000l"), b"\x1b[?1000l\x1b[?1006h");
        // Not private, not tracked, or not a set/reset: no effect.
        assert_eq!(modes(b"\x1b[4h\x1b[?12h\x1b[?25$p\x1b[?7l"), b"");
    }

    #[test]
    fn a_sequence_split_across_chunks_is_still_seen() {
        let mut t = ModeTracker::default();
        for chunk in [&b"\x1b"[..], b"[?10", b"49", b"h"] {
            t.feed(chunk);
        }
        assert_eq!(t.as_sequences(), b"\x1b[?1049h");
    }

    #[test]
    fn an_interrupted_sequence_does_not_leak_into_the_next() {
        // ESC inside a CSI starts over; the digits before it must not stick.
        assert_eq!(modes(b"\x1b[?20\x1b[?25l"), b"\x1b[?25l");
    }

    #[test]
    fn queries_are_stripped_and_everything_else_kept() {
        let q = |s: &[u8]| strip_queries(s);
        // Each of these asks the terminal something.
        for query in [
            &b"\x1b[6n"[..],
            b"\x1b[5n",
            b"\x1b[?6n",
            b"\x1b[c",
            b"\x1b[0c",
            b"\x1b[>c",
            b"\x1b[=c",
            b"\x1b[>q",
            b"\x1b[>0q",
            b"\x1b[?u",
            b"\x1b[?2004$p",
            b"\x1b[14t",
            b"\x1b[18t",
            b"\x1b]11;?\x07",
            b"\x1b]10;?\x1b\\",
            b"\x1b]4;1;?\x07",
            b"\x1b]52;c;?\x07",
            b"\x1bP$qm\x1b\\",
            b"\x1bP+q544e\x1b\\",
        ] {
            assert_eq!(q(&[b"a", query, b"b"].concat()), b"ab", "{:?}", String::from_utf8_lossy(query));
        }
        // These draw or set state, and must survive untouched.
        for keep in [
            &b"\x1b[31m"[..],
            b"\x1b[2J",
            b"\x1b[?1049h",
            b"\x1b[>1u",
            b"\x1b[<u",
            b"\x1b[2 q",
            b"\x1b[22;0t",
            b"\x1b[?62;22c",
            b"\x1b]0;title\x07",
            b"\x1b]11;#1e1e2e\x07",
            b"\x1b]52;c;aGk=\x07",
            b"\x1b]777;notify;T;done\x07",
            b"\x1bPq#0;2;0;0;0\x1b\\",
            "plain text, é".as_bytes(),
        ] {
            assert_eq!(q(keep), keep, "{:?}", String::from_utf8_lossy(keep));
        }
        // An unterminated sequence at the very end is left alone.
        assert_eq!(q(b"ok\x1b[6"), b"ok\x1b[6");
        assert_eq!(q(b"ok\x1b]11;?"), b"ok\x1b]11;?");
    }

    #[test]
    fn a_snapshot_never_asks_the_terminal_anything() {
        // ConPTY's opening question, then a shell prompt.
        let mut r = ReplayBuffer::new(1024);
        r.push(b"\x1b[6n\x1b[?25lPS C:\\> ");
        assert_eq!(r.snapshot(), b"\x1b[?25lPS C:\\> ");
    }

    #[test]
    fn keeps_everything_under_capacity() {
        let mut r = ReplayBuffer::new(100);
        r.push(b"hello\n");
        r.push(b"world");
        assert_eq!(r.snapshot(), b"hello\nworld");
    }

    #[test]
    fn trims_at_a_line_boundary() {
        let mut r = ReplayBuffer::new(10);
        r.push(b"aaaa\nbbbb\ncccc\n");
        // 15 bytes, cut at 5 lands on "bbbb\n" — already a line start.
        assert_eq!(r.snapshot(), b"bbbb\ncccc\n");
        r.push(b"dd");
        // Cut would fall inside "bbbb"; it moves past the next line feed.
        assert_eq!(r.snapshot(), b"cccc\ndd");
    }

    #[test]
    fn never_cuts_inside_an_escape_sequence() {
        // The plain cut would land inside the first sequence; the kept bytes
        // start at the next one instead.
        let mut r = ReplayBuffer::new(12);
        r.push(b"xxxx\x1b[38;5;196mred\x1b[0m ok");
        let snap = r.snapshot();
        assert_eq!(snap, b"\x1b[0m ok", "{:?}", String::from_utf8_lossy(&snap));
    }

    #[test]
    fn modes_set_before_the_window_are_restored_at_its_front() {
        let mut r = ReplayBuffer::new(64);
        r.push(b"\x1b[?1049h\x1b[?25l");
        for _ in 0..100 {
            r.push(b"redraw line\n");
        }
        let snap = r.snapshot();
        assert!(snap.starts_with(b"\x1b[?1049h\x1b[?25l"), "{:?}", String::from_utf8_lossy(&snap));
        assert!(r.len() <= 64);

        // A mode changed inside the window is replayed by the window itself,
        // not by the prefix.
        let mut r = ReplayBuffer::new(64);
        r.push(b"start\n");
        r.push(b"\x1b[?2004h");
        assert_eq!(r.snapshot(), b"start\n\x1b[?2004h");
    }

    #[test]
    fn gives_up_looking_for_a_safe_cut_eventually() {
        let mut r = ReplayBuffer::new(10);
        r.push(&vec![b'x'; 10 + SAFE_CUT_WINDOW + 100]);
        assert!(r.len() <= 10 + SAFE_CUT_WINDOW);
    }
}
