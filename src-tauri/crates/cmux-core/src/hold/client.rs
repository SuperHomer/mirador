//! Attaching to a holder: what the app (and the tests) use to talk to one.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;

use super::wire::{read_frame, write_frame, Frame, VERSION};

/// A live attachment, after the handshake and the replay.
pub struct Attached {
    /// The holder's protocol version: talk down to it if it is older.
    pub version: u16,
    pub pid: Option<u32>,
    /// `Some` if the child had already exited before this attach.
    pub exit_code: Option<i32>,
    /// What was printed before this attach, modes first.
    pub replay: Vec<u8>,
    /// Further frames: `Output`, then `Exited`. Write `Input`, `Resize`,
    /// `Kill` or `Detach` to it.
    pub stream: UnixStream,
}

pub fn attach(socket: &Path, cols: u16, rows: u16) -> io::Result<Attached> {
    let mut stream = UnixStream::connect(socket)?;
    write_frame(
        &mut stream,
        &Frame::ClientHello {
            version: VERSION,
            cols,
            rows,
        },
    )?;
    let Some(Frame::HolderHello {
        version,
        pid,
        exit_code,
    }) = read_frame(&mut stream)?
    else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "holder sent no hello"));
    };
    let Some(Frame::Replay(replay)) = read_frame(&mut stream)? else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "holder sent no replay"));
    };
    Ok(Attached {
        version,
        pid,
        exit_code,
        replay,
        stream,
    })
}
