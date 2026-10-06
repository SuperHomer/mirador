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

/// Starts `mira __hold` for a pane and returns once its socket accepts
/// connections. `mira` is the bundled CLI next to the app binary.
///
/// The holder is our child until we exit, so a thread waits on it: a
/// holder that ends while the app runs would otherwise linger as a zombie.
#[cfg(unix)]
pub fn launch(
    mira: &std::path::Path,
    pane: &str,
    socket: &std::path::Path,
    cwd: Option<&str>,
    cols: u16,
    rows: u16,
    program: &server::Program,
) -> std::io::Result<()> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut cmd = Command::new(mira);
    cmd.arg("__hold")
        .args(["--pane", pane])
        .arg("--socket")
        .arg(socket)
        .args(["--cols", &cols.to_string(), "--rows", &rows.to_string()]);
    if let Some(cwd) = cwd {
        cmd.args(["--cwd", cwd]);
    }
    match program {
        server::Program::Shell => {}
        server::Program::Command(line) => {
            cmd.args(["--command", line]);
        }
        server::Program::Ssh(host) => {
            cmd.args(["--ssh", host]);
        }
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if std::os::unix::net::UnixStream::connect(socket).is_ok() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            // Never leave a holder behind that nobody can reach.
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("holder for {pane} never opened {}", socket.display()),
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Whether a live holder answers on this pane's socket.
#[cfg(unix)]
pub fn is_alive(pane_id: &str) -> bool {
    std::os::unix::net::UnixStream::connect(socket_path(pane_id)).is_ok()
}
