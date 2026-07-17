//! Raw-stream scanning. Every PTY reader pushes its chunks through a
//! `StreamScanner` before forwarding them to the UI.
//!
//! `OscScanner` is a persistent byte state machine (sequences can be split
//! across read chunks arbitrarily) that:
//! - strips and emits notification sequences: OSC 9 (iTerm2), OSC 99
//!   (kitty, single-shot form), OSC 777 (`notify;title;body`)
//! - observes without stripping: OSC 7 (cwd), OSC 0/2 (window title)
//! - passes every other byte through untouched.

use std::borrow::Cow;

pub trait StreamScanner: Send {
    /// Scan one chunk of raw PTY output and return the bytes to forward.
    fn scan<'a>(&mut self, chunk: &'a [u8]) -> Cow<'a, [u8]>;
}

/// Forwards everything untouched (tests / non-interactive panes).
pub struct PassthroughScanner;

impl StreamScanner for PassthroughScanner {
    fn scan<'a>(&mut self, chunk: &'a [u8]) -> Cow<'a, [u8]> {
        Cow::Borrowed(chunk)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OscEvent {
    Notification { title: Option<String>, body: String },
    Cwd(String),
    Title(String),
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Ground,
    /// Saw ESC.
    Esc,
    /// Inside `ESC ]` collecting parameter bytes.
    Osc,
    /// Inside OSC, saw ESC (potential ST terminator).
    OscEsc,
    /// Oversized OSC: stream through until terminator, don't parse.
    OscOverflow,
    /// Oversized OSC, saw ESC.
    OscOverflowEsc,
}

/// Upper bound on a buffered OSC sequence; beyond this we stop buffering
/// and pass the rest through (protects against `cat /dev/urandom`).
const MAX_OSC: usize = 64 * 1024;

pub struct OscScanner<F: FnMut(OscEvent) + Send> {
    on_event: F,
    state: State,
    /// Raw bytes of the sequence being buffered (incl. `ESC ]`).
    raw: Vec<u8>,
    /// Parameter bytes only (between `ESC ]` and the terminator).
    data: Vec<u8>,
}

impl<F: FnMut(OscEvent) + Send> OscScanner<F> {
    pub fn new(on_event: F) -> Self {
        Self {
            on_event,
            state: State::Ground,
            raw: Vec::new(),
            data: Vec::new(),
        }
    }

    /// Parses the completed sequence. Returns true when the sequence was
    /// consumed (stripped from the stream).
    fn handle_sequence(&mut self) -> bool {
        let text = String::from_utf8_lossy(&self.data).into_owned();
        let (num, rest) = match text.split_once(';') {
            Some((n, r)) => (n, r),
            None => (text.as_str(), ""),
        };
        match num {
            "9" => {
                (self.on_event)(OscEvent::Notification {
                    title: None,
                    body: rest.to_string(),
                });
                true
            }
            "777" => {
                if let Some(payload) = rest.strip_prefix("notify;") {
                    let (title, body) = match payload.split_once(';') {
                        Some((t, b)) => (Some(t.to_string()), b.to_string()),
                        None => (None, payload.to_string()),
                    };
                    (self.on_event)(OscEvent::Notification { title, body });
                    true
                } else {
                    false
                }
            }
            "99" => {
                // kitty desktop notification, single-shot form:
                // OSC 99 ; metadata ; payload ST  (metadata is k=v pairs
                // joined by ':'; e=1 means base64 payload; p names the part)
                let (meta, payload) = match rest.split_once(';') {
                    Some((m, p)) => (m, p),
                    None => ("", rest),
                };
                let mut is_title = false;
                let mut base64 = false;
                for kv in meta.split(':') {
                    match kv.split_once('=') {
                        Some(("p", "title")) => is_title = true,
                        Some(("e", "1")) => base64 = true,
                        _ => {}
                    }
                }
                let decoded = if base64 {
                    decode_base64(payload.trim()).unwrap_or_else(|| payload.to_string())
                } else {
                    payload.to_string()
                };
                if !decoded.is_empty() {
                    let (title, body) = if is_title {
                        (Some(decoded), String::new())
                    } else {
                        (None, decoded)
                    };
                    (self.on_event)(OscEvent::Notification { title, body });
                }
                true
            }
            "7" => {
                if let Some(path) = parse_file_url(rest) {
                    (self.on_event)(OscEvent::Cwd(path));
                }
                false
            }
            "0" | "2" => {
                (self.on_event)(OscEvent::Title(rest.to_string()));
                false
            }
            _ => false,
        }
    }

    fn finish(&mut self, out: &mut Vec<u8>) {
        let strip = self.handle_sequence();
        if !strip {
            out.extend_from_slice(&self.raw);
        }
        self.raw.clear();
        self.data.clear();
        self.state = State::Ground;
    }
}

impl<F: FnMut(OscEvent) + Send> StreamScanner for OscScanner<F> {
    fn scan<'a>(&mut self, chunk: &'a [u8]) -> Cow<'a, [u8]> {
        // Fast path: nothing pending and no ESC anywhere in the chunk.
        if self.state == State::Ground && !chunk.contains(&0x1b) {
            return Cow::Borrowed(chunk);
        }

        let mut out = Vec::with_capacity(chunk.len());
        for &b in chunk {
            match self.state {
                State::Ground => {
                    if b == 0x1b {
                        self.state = State::Esc;
                        self.raw.clear();
                        self.raw.push(b);
                    } else {
                        out.push(b);
                    }
                }
                State::Esc => {
                    if b == b']' {
                        self.state = State::Osc;
                        self.raw.push(b);
                        self.data.clear();
                    } else if b == 0x1b {
                        // Lone ESC followed by another ESC: flush the first.
                        out.push(0x1b);
                        self.raw.clear();
                        self.raw.push(b);
                    } else {
                        // Not an OSC introducer: flush both bytes untouched.
                        out.push(0x1b);
                        out.push(b);
                        self.state = State::Ground;
                    }
                }
                State::Osc => {
                    self.raw.push(b);
                    if b == 0x07 {
                        self.finish(&mut out);
                    } else if b == 0x1b {
                        self.state = State::OscEsc;
                    } else {
                        self.data.push(b);
                        if self.raw.len() > MAX_OSC {
                            // Give up on parsing: flush what we have and
                            // stream the rest until the terminator.
                            out.extend_from_slice(&self.raw);
                            self.raw.clear();
                            self.data.clear();
                            self.state = State::OscOverflow;
                        }
                    }
                }
                State::OscEsc => {
                    self.raw.push(b);
                    if b == b'\\' {
                        self.finish(&mut out);
                    } else {
                        // ESC inside the payload: keep it as data.
                        self.data.push(0x1b);
                        self.data.push(b);
                        self.state = State::Osc;
                    }
                }
                State::OscOverflow => {
                    out.push(b);
                    if b == 0x07 {
                        self.state = State::Ground;
                    } else if b == 0x1b {
                        self.state = State::OscOverflowEsc;
                    }
                }
                State::OscOverflowEsc => {
                    out.push(b);
                    self.state = if b == b'\\' {
                        State::Ground
                    } else {
                        State::OscOverflow
                    };
                }
            }
        }
        Cow::Owned(out)
    }
}

/// Reduces raw terminal output to agent-readable plain text: drops escape
/// sequences (CSI/OSC/simple), resolves carriage-return overwrites
/// (progress bars keep only their final state), normalizes CRLF.
pub fn strip_ansi(bytes: &[u8]) -> String {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut line: Vec<u8> = Vec::new();
    let mut cr = false; // pending carriage return (overwrite line unless \n follows)
    let mut i = 0;
    let flush_cr = |line: &mut Vec<u8>, cr: &mut bool| {
        if *cr {
            line.clear();
            *cr = false;
        }
    };
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            0x1b => {
                i += 1;
                match bytes.get(i) {
                    Some(b'[') => {
                        // CSI: parameters then a final byte in 0x40..=0x7e
                        i += 1;
                        while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                            i += 1;
                        }
                        i += 1;
                    }
                    Some(b']') => {
                        // OSC: until BEL or ESC \
                        i += 1;
                        while i < bytes.len() {
                            if bytes[i] == 0x07 {
                                i += 1;
                                break;
                            }
                            if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'\\') {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                    }
                    _ => i += 1, // simple escape: skip one byte
                }
                continue;
            }
            b'\r' => cr = true,
            b'\n' => {
                cr = false;
                out.push(std::mem::take(&mut line));
            }
            0x08 => {
                flush_cr(&mut line, &mut cr);
                line.pop();
            }
            _ => {
                flush_cr(&mut line, &mut cr);
                if b >= 0x20 || b == b'\t' {
                    line.push(b);
                }
            }
        }
        i += 1;
    }
    if !line.is_empty() {
        out.push(line);
    }
    let joined = out.join(&b'\n');
    String::from_utf8_lossy(&joined).into_owned()
}

/// `file://host/path` → percent-decoded path.
fn parse_file_url(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file://")?;
    let path_start = rest.find('/')?;
    Some(percent_decode(&rest[path_start..]))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn decode_base64(s: &str) -> Option<String> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut lookup = [255u8; 256];
    for (i, &c) in TABLE.iter().enumerate() {
        lookup[c as usize] = i as u8;
    }
    let mut out = Vec::new();
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &c in s.as_bytes() {
        if c == b'=' || c == b'\n' || c == b'\r' {
            continue;
        }
        let v = lookup[c as usize];
        if v == 255 {
            return None;
        }
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    type EventLog = Arc<Mutex<Vec<OscEvent>>>;

    fn scanner_with_events() -> (OscScanner<impl FnMut(OscEvent) + Send>, EventLog) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let scanner = OscScanner::new(move |e| sink.lock().unwrap().push(e));
        (scanner, events)
    }

    fn scan_all(scanner: &mut impl StreamScanner, input: &[u8]) -> Vec<u8> {
        scanner.scan(input).into_owned()
    }

    fn scan_byte_by_byte(scanner: &mut impl StreamScanner, input: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for b in input {
            out.extend_from_slice(&scanner.scan(std::slice::from_ref(b)));
        }
        out
    }

    #[test]
    fn plain_text_passes_through_borrowed() {
        let (mut s, events) = scanner_with_events();
        assert_eq!(scan_all(&mut s, b"hello world"), b"hello world");
        assert!(events.lock().unwrap().is_empty());
    }

    #[test]
    fn osc9_stripped_and_emitted() {
        let (mut s, events) = scanner_with_events();
        let out = scan_all(&mut s, b"before\x1b]9;Build done\x07after");
        assert_eq!(out, b"beforeafter");
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[OscEvent::Notification {
                title: None,
                body: "Build done".into()
            }]
        );
    }

    #[test]
    fn osc777_with_title_and_st_terminator() {
        let (mut s, events) = scanner_with_events();
        let out = scan_all(&mut s, b"\x1b]777;notify;Tests;3 failed\x1b\\tail");
        assert_eq!(out, b"tail");
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &[OscEvent::Notification {
                title: Some("Tests".into()),
                body: "3 failed".into()
            }]
        );
    }

    #[test]
    fn osc99_kitty_plain_and_base64() {
        let (mut s, events) = scanner_with_events();
        scan_all(&mut s, b"\x1b]99;;Agent needs input\x1b\\");
        scan_all(&mut s, b"\x1b]99;e=1;SGVsbG8=\x07");
        let evs = events.lock().unwrap();
        assert_eq!(
            evs[0],
            OscEvent::Notification {
                title: None,
                body: "Agent needs input".into()
            }
        );
        assert_eq!(
            evs[1],
            OscEvent::Notification {
                title: None,
                body: "Hello".into()
            }
        );
    }

    #[test]
    fn split_across_chunks_byte_by_byte() {
        let (mut s, events) = scanner_with_events();
        let out = scan_byte_by_byte(&mut s, b"a\x1b]777;notify;T;B\x1b\\z");
        assert_eq!(out, b"az");
        assert_eq!(events.lock().unwrap().len(), 1);
    }

    #[test]
    fn title_and_cwd_observed_but_not_stripped() {
        let (mut s, events) = scanner_with_events();
        let seq = b"\x1b]2;my title\x07\x1b]7;file://mac.local/Users/yo%20an\x1b\\";
        let out = scan_all(&mut s, seq);
        assert_eq!(out, seq, "title/cwd sequences must pass through");
        let evs = events.lock().unwrap();
        assert_eq!(evs[0], OscEvent::Title("my title".into()));
        assert_eq!(evs[1], OscEvent::Cwd("/Users/yo an".into()));
    }

    #[test]
    fn unrelated_osc_passes_through_unchanged() {
        let (mut s, events) = scanner_with_events();
        let seq = b"\x1b]10;#ffffff\x07";
        assert_eq!(scan_all(&mut s, seq), seq);
        assert!(events.lock().unwrap().is_empty());
    }

    #[test]
    fn non_osc_escapes_untouched() {
        let (mut s, events) = scanner_with_events();
        let seq = b"\x1b[31mred\x1b[0m";
        assert_eq!(scan_all(&mut s, seq), seq);
        assert_eq!(scan_byte_by_byte(&mut s, seq), seq);
        assert!(events.lock().unwrap().is_empty());
    }

    #[test]
    fn oversized_osc_streams_through() {
        let (mut s, events) = scanner_with_events();
        let mut seq = b"\x1b]1337;".to_vec();
        seq.extend(std::iter::repeat_n(b'x', MAX_OSC + 100));
        seq.push(0x07);
        seq.extend_from_slice(b"tail");
        let out = scan_all(&mut s, &seq);
        assert_eq!(out, seq, "oversized sequences must not be swallowed");
        assert!(events.lock().unwrap().is_empty());
        // Scanner recovered: normal parsing resumes.
        let out = scan_all(&mut s, b"\x1b]9;ok\x07");
        assert_eq!(out, b"");
        assert_eq!(events.lock().unwrap().len(), 1);
    }

    #[test]
    fn strip_ansi_cleans_terminal_output() {
        // colors + cursor moves dropped
        assert_eq!(strip_ansi(b"\x1b[31mred\x1b[0m ok\x1b[2K"), "red ok");
        // CRLF and plain LF normalize
        assert_eq!(strip_ansi(b"line1\r\nline2\nline3"), "line1\nline2\nline3");
        // progress-bar overwrites keep the final state
        assert_eq!(strip_ansi(b"10%\r20%\r100%"), "100%");
        // OSC stripped
        assert_eq!(strip_ansi(b"\x1b]2;title\x07text"), "text");
        // utf-8 survives
        assert_eq!(strip_ansi("héllo ✓".as_bytes()), "héllo ✓");
    }

    #[test]
    fn double_esc_then_osc() {
        let (mut s, events) = scanner_with_events();
        let out = scan_all(&mut s, b"\x1b\x1b]9;n\x07");
        // First ESC flushed; second starts a real OSC 9 that is stripped.
        assert_eq!(out, b"\x1b");
        assert_eq!(events.lock().unwrap().len(), 1);
    }
}
