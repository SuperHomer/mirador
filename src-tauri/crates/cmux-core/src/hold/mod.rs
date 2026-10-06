//! Session holders: one process per pane that owns its PTY and child, so
//! terminals keep running when the app quits and a relaunch reattaches to
//! them. Design and rationale: docs/design/session-persistence.md.
//!
//! A holder listens on a Unix socket (macOS, Linux) or a named pipe
//! (Windows) — see [`conn`]. Where those live is a *location*: a directory
//! of sockets, or a pipe-name prefix.

pub mod client;
pub mod conn;
pub mod replay;
pub mod server;
pub mod wire;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

/// Where this user's holders live.
///
/// On unix, a directory: under `$XDG_RUNTIME_DIR` when it is set, so a
/// sandbox instance with its own runtime dir can never see — let alone
/// attach — the real app's live shells. Otherwise `~/.mirador/holders`:
/// the data directory (`~/Library/Application Support/Mirador` on macOS)
/// puts a socket path at ~100 bytes, against a 104-byte limit, and the temp
/// directory is swept of files untouched for days, which would cut a
/// long-lived session off from its socket.
///
/// On Windows, a pipe-name prefix carrying the user name, like the
/// automation pipe's: named pipes live in one machine-wide namespace.
pub fn location() -> PathBuf {
    #[cfg(unix)]
    {
        match std::env::var("XDG_RUNTIME_DIR") {
            Ok(dir) if !dir.is_empty() => PathBuf::from(dir).join("mirador").join("holders"),
            _ => PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
                .join(".mirador")
                .join("holders"),
        }
    }
    #[cfg(windows)]
    {
        let user: String = std::env::var("USERNAME")
            .unwrap_or_default()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        PathBuf::from(format!(r"\\.\pipe\mirador-hold-{user}"))
    }
}

/// A pane's holder endpoint. Pane ids are uuids we generate; still, never
/// trust one as a path component.
pub fn socket_path(pane_id: &str) -> PathBuf {
    conn::endpoint(&location(), &safe_id(pane_id))
}

fn safe_id(pane_id: &str) -> String {
    pane_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect()
}

/// Starts `mira __hold` for a pane and returns once it accepts
/// connections. `mira` is the bundled CLI next to the app binary.
pub fn launch(
    mira: &Path,
    pane: &str,
    socket: &Path,
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
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = spawn_detached(&mut cmd)?;

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if conn::is_alive(socket) {
            // The holder is our child until we exit: on unix a thread
            // waits on it, or one that ends while the app runs would linger
            // as a zombie. Windows has no zombies; the handle just closes.
            #[cfg(unix)]
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(());
        }
        if Instant::now() >= deadline || matches!(child.try_wait(), Ok(Some(_))) {
            // Never leave a holder behind that nobody can reach.
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("holder for {pane} never opened {}", socket.display()),
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// On unix the holder detaches itself (`setsid` in `mira __hold`). On
/// Windows it has to be started detached: no console, its own process
/// group, and — where the job the app runs in allows it — outside that
/// job, so closing the app's job cannot take the holders with it.
fn spawn_detached(cmd: &mut std::process::Command) -> std::io::Result<std::process::Child> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{
            CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS,
        };
        let detached = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        match cmd.creation_flags(detached | CREATE_BREAKAWAY_FROM_JOB).spawn() {
            Ok(child) => return Ok(child),
            // A job that forbids breakaway refuses the flag outright; stay
            // in it rather than not start at all.
            Err(e) if e.raw_os_error() == Some(5) => {}
            Err(e) => return Err(e),
        }
        cmd.creation_flags(detached).spawn()
    }
    #[cfg(not(windows))]
    {
        cmd.spawn()
    }
}

/// Whether a live holder answers for this pane.
pub fn is_alive(pane_id: &str) -> bool {
    conn::is_alive(&socket_path(pane_id))
}

/// Pane ids with a live holder at this user's location.
pub fn live_panes() -> Vec<String> {
    conn::live_panes(&location())
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

/// Ends every holder at this user's location — attached or not, orphaned
/// or not — and waits up to `patience` for them to go. For "Quit and end
/// all sessions". Returns the panes whose holder was still answering at the
/// deadline.
pub fn end_all(patience: std::time::Duration) -> Vec<String> {
    end_all_in(&location(), patience)
}

/// Each connection stays open until its holder reports the exit: a holder
/// whose child dies with nobody attached waits a day for someone to
/// collect it, so hanging up straight after `Kill` would leave it there.
fn end_all_in(location: &Path, patience: std::time::Duration) -> Vec<String> {
    use std::time::Instant;
    let deadline = Instant::now() + patience;
    let panes = conn::live_panes(location);
    let mut attached = Vec::new();
    for pane in &panes {
        // Attaching takes the pane over from the app, which is quitting.
        if let Ok(mut a) = client::attach(&conn::endpoint(location, pane), 80, 24) {
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
    // A holder goes away as it exits; give the last ones a moment.
    loop {
        let left: Vec<String> = panes
            .iter()
            .filter(|p| conn::is_alive(&conn::endpoint(location, p)))
            .cloned()
            .collect();
        if left.is_empty() || Instant::now() >= deadline {
            return left;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
