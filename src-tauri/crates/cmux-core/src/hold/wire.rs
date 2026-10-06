//! The holder's wire protocol: length-prefixed frames over a Unix socket.
//!
//! This is the one interface that has to stay compatible across releases —
//! after an in-place update the new app talks to holders still running the
//! old binary — so it is deliberately small, and it only ever grows:
//! a frame type nobody knows is skipped, never an error.
//!
//! A frame is `[u32 length, big-endian][u8 type][payload]`, where `length`
//! counts the type byte and the payload.

use std::io::{self, Read, Write};

/// Bumped only for a change an older peer cannot ignore.
pub const VERSION: u16 = 1;

/// Comfortably above the largest frame we send (a 4 MB replay plus its mode
/// prefix); anything bigger is a corrupt stream, not a frame.
const MAX_FRAME: u32 = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// Client → holder, first frame: who is attaching and at what size.
    ClientHello { version: u16, cols: u16, rows: u16 },
    /// Holder → client, first frame: what it holds.
    HolderHello {
        version: u16,
        /// The child's pid, for the app's cwd/port/agent lookups.
        pid: Option<u32>,
        /// Present once the child has exited.
        exit_code: Option<i32>,
    },
    /// Client → holder: keystrokes for the child.
    Input(Vec<u8>),
    /// Holder → client: live output.
    Output(Vec<u8>),
    /// Holder → client, once after `HolderHello`: what was printed before
    /// this attach, prefixed with the terminal modes in force at its start.
    Replay(Vec<u8>),
    /// Client → holder.
    Resize { cols: u16, rows: u16 },
    /// Holder → client: the child exited (code unknown if killed by signal).
    Exited(Option<i32>),
    /// Client → holder: end the child's whole process group.
    Kill,
    /// Client → holder: going away; keep running.
    Detach,
}

const CLIENT_HELLO: u8 = 1;
const HOLDER_HELLO: u8 = 2;
const INPUT: u8 = 3;
const OUTPUT: u8 = 4;
const REPLAY: u8 = 5;
const RESIZE: u8 = 6;
const EXITED: u8 = 7;
const KILL: u8 = 8;
const DETACH: u8 = 9;

impl Frame {
    fn encode(&self) -> (u8, Vec<u8>) {
        match self {
            Frame::ClientHello {
                version,
                cols,
                rows,
            } => {
                let mut p = Vec::with_capacity(6);
                p.extend_from_slice(&version.to_be_bytes());
                p.extend_from_slice(&cols.to_be_bytes());
                p.extend_from_slice(&rows.to_be_bytes());
                (CLIENT_HELLO, p)
            }
            Frame::HolderHello {
                version,
                pid,
                exit_code,
            } => {
                let mut p = Vec::with_capacity(12);
                p.extend_from_slice(&version.to_be_bytes());
                p.extend_from_slice(&pid.unwrap_or(0).to_be_bytes());
                push_code(&mut p, *exit_code);
                (HOLDER_HELLO, p)
            }
            Frame::Input(b) => (INPUT, b.clone()),
            Frame::Output(b) => (OUTPUT, b.clone()),
            Frame::Replay(b) => (REPLAY, b.clone()),
            Frame::Resize { cols, rows } => {
                let mut p = Vec::with_capacity(4);
                p.extend_from_slice(&cols.to_be_bytes());
                p.extend_from_slice(&rows.to_be_bytes());
                (RESIZE, p)
            }
            Frame::Exited(code) => {
                let mut p = Vec::with_capacity(5);
                push_code(&mut p, *code);
                (EXITED, p)
            }
            Frame::Kill => (KILL, Vec::new()),
            Frame::Detach => (DETACH, Vec::new()),
        }
    }

    /// `None` for a type this version doesn't know: skip it.
    fn decode(kind: u8, p: Vec<u8>) -> io::Result<Option<Frame>> {
        let mut r = Payload(&p);
        let frame = match kind {
            CLIENT_HELLO => Frame::ClientHello {
                version: r.u16()?,
                cols: r.u16()?,
                rows: r.u16()?,
            },
            HOLDER_HELLO => Frame::HolderHello {
                version: r.u16()?,
                pid: Some(r.u32()?).filter(|&pid| pid != 0),
                exit_code: r.code()?,
            },
            INPUT => Frame::Input(p),
            OUTPUT => Frame::Output(p),
            REPLAY => Frame::Replay(p),
            RESIZE => Frame::Resize {
                cols: r.u16()?,
                rows: r.u16()?,
            },
            EXITED => Frame::Exited(r.code()?),
            KILL => Frame::Kill,
            DETACH => Frame::Detach,
            _ => return Ok(None),
        };
        Ok(Some(frame))
    }
}

fn push_code(p: &mut Vec<u8>, code: Option<i32>) {
    p.push(code.is_some() as u8);
    p.extend_from_slice(&code.unwrap_or(0).to_be_bytes());
}

/// Reads fields off a payload. Trailing bytes are allowed: a newer peer may
/// append fields this version doesn't know.
struct Payload<'a>(&'a [u8]);

impl Payload<'_> {
    fn take<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        if self.0.len() < N {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "short frame"));
        }
        let (head, rest) = self.0.split_at(N);
        self.0 = rest;
        Ok(head.try_into().unwrap())
    }
    fn u16(&mut self) -> io::Result<u16> {
        self.take().map(u16::from_be_bytes)
    }
    fn u32(&mut self) -> io::Result<u32> {
        self.take().map(u32::from_be_bytes)
    }
    fn code(&mut self) -> io::Result<Option<i32>> {
        let [present] = self.take()?;
        let code = i32::from_be_bytes(self.take()?);
        Ok((present != 0).then_some(code))
    }
}

pub fn write_frame(w: &mut impl Write, frame: &Frame) -> io::Result<()> {
    let (kind, payload) = frame.encode();
    let len = payload.len() as u32 + 1;
    // One write per frame, so concurrent writers holding the same lock
    // never interleave and a reader never sees half a header.
    let mut buf = Vec::with_capacity(5 + payload.len());
    buf.extend_from_slice(&len.to_be_bytes());
    buf.push(kind);
    buf.extend_from_slice(&payload);
    w.write_all(&buf)?;
    w.flush()
}

/// The next frame this version understands; `Ok(None)` at a clean EOF.
pub fn read_frame(r: &mut impl Read) -> io::Result<Option<Frame>> {
    loop {
        let mut len = [0u8; 4];
        match r.read_exact(&mut len) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }
        let len = u32::from_be_bytes(len);
        if len == 0 || len > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("bad frame length {len}"),
            ));
        }
        let mut body = vec![0u8; len as usize];
        r.read_exact(&mut body)?;
        let kind = body[0];
        body.remove(0);
        if let Some(frame) = Frame::decode(kind, body)? {
            return Ok(Some(frame));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(frame: Frame) {
        let mut buf = Vec::new();
        write_frame(&mut buf, &frame).unwrap();
        let back = read_frame(&mut buf.as_slice()).unwrap();
        assert_eq!(back, Some(frame));
    }

    #[test]
    fn every_frame_roundtrips() {
        roundtrip(Frame::ClientHello { version: VERSION, cols: 120, rows: 40 });
        roundtrip(Frame::HolderHello { version: VERSION, pid: Some(4242), exit_code: None });
        roundtrip(Frame::HolderHello { version: VERSION, pid: None, exit_code: Some(-1) });
        roundtrip(Frame::Input(b"ls\r".to_vec()));
        roundtrip(Frame::Output(vec![0x1b, b'[', b'm', 0xff]));
        roundtrip(Frame::Replay(Vec::new()));
        roundtrip(Frame::Resize { cols: 80, rows: 24 });
        roundtrip(Frame::Exited(Some(0)));
        roundtrip(Frame::Exited(None));
        roundtrip(Frame::Kill);
        roundtrip(Frame::Detach);
    }

    #[test]
    fn unknown_frames_and_trailing_fields_are_skipped() {
        let mut buf = Vec::new();
        // A frame type from the future, then a hello with an extra field.
        buf.extend_from_slice(&3u32.to_be_bytes());
        buf.extend_from_slice(&[200, 1, 2]);
        buf.extend_from_slice(&8u32.to_be_bytes());
        buf.push(RESIZE);
        buf.extend_from_slice(&[0, 80, 0, 24, 9, 9, 9]);
        write_frame(&mut buf, &Frame::Detach).unwrap();

        let mut r = buf.as_slice();
        assert_eq!(read_frame(&mut r).unwrap(), Some(Frame::Resize { cols: 80, rows: 24 }));
        assert_eq!(read_frame(&mut r).unwrap(), Some(Frame::Detach));
        assert_eq!(read_frame(&mut r).unwrap(), None);
    }

    #[test]
    fn corrupt_streams_are_errors_not_hangs() {
        let mut huge = Vec::new();
        huge.extend_from_slice(&(MAX_FRAME + 1).to_be_bytes());
        assert!(read_frame(&mut huge.as_slice()).is_err());

        let mut zero = 0u32.to_be_bytes().to_vec();
        zero.push(KILL);
        assert!(read_frame(&mut zero.as_slice()).is_err());

        // A hello cut short.
        let mut short = 2u32.to_be_bytes().to_vec();
        short.extend_from_slice(&[HOLDER_HELLO, 0]);
        assert!(read_frame(&mut short.as_slice()).is_err());
    }
}
