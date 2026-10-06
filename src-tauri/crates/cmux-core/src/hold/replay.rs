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
        out.extend(self.bytes.iter());
        out
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
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
