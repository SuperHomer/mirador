//! Raw-stream scanning infrastructure. Every PTY reader thread pushes its
//! chunks through a `StreamScanner` before forwarding them to the UI, so
//! later milestones (M4 notifications) can observe/strip OSC sequences
//! without re-plumbing the data path.

use std::borrow::Cow;

pub trait StreamScanner: Send {
    /// Scan one chunk of raw PTY output and return the bytes to forward.
    /// Implementations may strip sequences they consume; they must be
    /// persistent state machines since escape sequences can be split
    /// across chunks.
    fn scan<'a>(&mut self, chunk: &'a [u8]) -> Cow<'a, [u8]>;
}

/// Forwards everything untouched. Placeholder until the M4 OSC scanner.
pub struct PassthroughScanner;

impl StreamScanner for PassthroughScanner {
    fn scan<'a>(&mut self, chunk: &'a [u8]) -> Cow<'a, [u8]> {
        Cow::Borrowed(chunk)
    }
}
