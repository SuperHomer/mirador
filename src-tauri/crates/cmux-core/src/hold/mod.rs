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

/// Pane ids with a live holder in `holders_dir()`. Socket files whose
/// holder is gone are removed on the way, so the directory doesn't
/// accumulate them.
#[cfg(unix)]
pub fn live_panes() -> Vec<String> {
    live_panes_in(&holders_dir())
}

#[cfg(unix)]
fn live_panes_in(dir: &std::path::Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut panes = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(pane) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".sock"))
        else {
            continue;
        };
        if std::os::unix::net::UnixStream::connect(&path).is_ok() {
            panes.push(pane.to_string());
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
    panes.sort();
    panes
}

/// Live holders no pane in the session names: what is left when the app
/// crashed, or died before saving a pane it had just opened. Reopened
/// rather than killed — those are exactly the cases where the holder has
/// work the user wants back.
pub fn orphans(live: &[String], session_panes: &[String]) -> Vec<String> {
    live.iter()
        .filter(|p| !session_panes.contains(p))
        .cloned()
        .collect()
}

/// Ends every holder in `holders_dir()` — attached or not, orphaned or not
/// — and waits up to `patience` for them to go. For "Quit and end all
/// sessions". Returns the panes whose holder was still answering at the
/// deadline.
///
/// Each connection stays open until its holder reports the exit: a holder
/// whose child dies with nobody attached waits a day for someone to
/// collect it, so hanging up straight after `Kill` would leave it there.
#[cfg(unix)]
pub fn end_all(patience: std::time::Duration) -> Vec<String> {
    end_all_in(&holders_dir(), patience)
}

#[cfg(unix)]
fn end_all_in(dir: &std::path::Path, patience: std::time::Duration) -> Vec<String> {
    use std::time::Instant;
    let deadline = Instant::now() + patience;
    let panes = live_panes_in(dir);
    let sock = |pane: &str| dir.join(format!("{pane}.sock"));
    let mut attached = Vec::new();
    for pane in &panes {
        // Attaching takes the pane over from the app, which is quitting.
        if let Ok(mut a) = client::attach(&sock(pane), 80, 24) {
            if wire::write_frame(&mut a.stream, &wire::Frame::Kill).is_ok() {
                attached.push(a);
            }
        }
    }
    for mut a in attached {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || a.stream.set_read_timeout(Some(left)).is_err() {
                break;
            }
            match wire::read_frame(&mut a.stream) {
                Ok(Some(wire::Frame::Exited(_))) | Ok(None) | Err(_) => break,
                Ok(Some(_)) => {}
            }
        }
    }
    // The holder removes its socket as it exits; give the last ones a moment.
    loop {
        let left: Vec<String> = panes
            .iter()
            .filter(|p| std::os::unix::net::UnixStream::connect(sock(p)).is_ok())
            .cloned()
            .collect();
        if left.is_empty() || Instant::now() >= deadline {
            return left;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
