//! Session holders: one process per pane that owns its PTY and child, so
//! terminals keep running when the app quits and a relaunch reattaches to
//! them. Design and rationale: docs/design/session-persistence.md.
//!
//! The protocol and the replay buffer are platform-neutral; the holder and
//! its client are unix-only until Windows gets named pipes (phase 2).

pub mod replay;
pub mod wire;

#[cfg(unix)]
pub mod client;
#[cfg(unix)]
pub mod server;
#[cfg(all(unix, test))]
mod tests;

use std::path::PathBuf;

/// Where holder sockets live. Under `$XDG_RUNTIME_DIR` when it is set, so a
/// sandbox instance with its own runtime dir can never see — let alone
/// attach — the real app's live shells. Otherwise `~/.mirador/holders`:
/// the data directory (`~/Library/Application Support/Mirador` on macOS)
/// puts a socket path at ~100 bytes, against a 104-byte limit, and the temp
/// directory is swept of files untouched for days, which would cut a
/// long-lived session off from its socket.
pub fn holders_dir() -> PathBuf {
    match std::env::var("XDG_RUNTIME_DIR") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir).join("mirador").join("holders"),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
            .join(".mirador")
            .join("holders"),
    }
}

/// A pane's holder socket. Pane ids are uuids we generate; still, never
/// trust one as a path component.
pub fn socket_path(pane_id: &str) -> PathBuf {
    let safe: String = pane_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    holders_dir().join(format!("{safe}.sock"))
}
