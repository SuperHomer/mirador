//! The holder: one pane's PTY and child, kept alive in a process of its own
//! so they outlive the app, and served to whichever client attaches.
//!
//! It is a `PtyManager` with one pane, a [`ReplayBuffer`] of its output and
//! a Unix socket. Output goes into the buffer always, and to the attached
//! client when there is one. A client that attaches gets a `HolderHello`,
//! then everything in the buffer as one `Replay`, then live `Output` — all
//! under one lock, so nothing printed in between is lost or sent twice.

use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::osc::PassthroughScanner;
use crate::pty::PtyManager;

use super::replay::ReplayBuffer;
use super::wire::{read_frame, write_frame, Frame, VERSION};

/// What the pane runs.
#[derive(Debug, Clone)]
pub enum Program {
    /// The user's interactive login shell.
    Shell,
    /// A command pane: one command line, handed to the shell.
    Command(String),
    /// A remote pane: `ssh -tt <host>`.
    Ssh(String),
}

#[derive(Debug, Clone)]
pub struct HoldConfig {
    pub pane: String,
    pub socket: PathBuf,
    pub cwd: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub program: Program,
    /// How much output a client gets back on attach.
    pub replay_bytes: usize,
    /// How long an exited child's holder waits for a client to collect its
    /// output and exit code before giving up.
    pub exited_grace: Duration,
}

impl HoldConfig {
    pub const DEFAULT_REPLAY_BYTES: usize = 4 * 1024 * 1024;
    pub const DEFAULT_EXITED_GRACE: Duration = Duration::from_secs(24 * 60 * 60);
}

struct Client {
    id: u64,
    stream: UnixStream,
}

struct Shared {
    replay: ReplayBuffer,
    client: Option<Client>,
    /// `Some` once the child has exited, holding its code.
    exited: Option<Option<i32>>,
}

enum Event {
    /// The child exited.
    Exited,
    /// A client has been told the child exited: the holder's job is done.
    Collected,
}

/// Runs a holder until its child has exited and a client has seen that (or
/// nobody came within `exited_grace`). Returns the child's exit code.
///
/// Blocks the calling thread; `mira __hold` calls it after `setsid` so the
/// holder belongs to no terminal and survives whoever started it.
pub fn serve(config: HoldConfig) -> io::Result<Option<i32>> {
    let listener = bind(&config.socket)?;
    let pty = PtyManager::unthrottled();
    let shared = Arc::new(Mutex::new(Shared {
        replay: ReplayBuffer::new(config.replay_bytes),
        client: None,
        exited: None,
    }));
    let (events, events_rx) = mpsc::channel::<Event>();

    let command = match &config.program {
        Program::Shell => None,
        Program::Command(cmd) => Some(crate::pty::shell::run_command(cmd, config.cwd.as_deref())),
        Program::Ssh(host) => Some(crate::ssh::interactive_command(host)),
    };
    let on_data = {
        let shared = Arc::clone(&shared);
        move |bytes: &[u8]| {
            let mut s = shared.lock().unwrap();
            s.replay.push(bytes);
            send_to_client(&mut s, &Frame::Output(bytes.to_vec()));
        }
    };
    let on_exit = {
        let shared = Arc::clone(&shared);
        let events = events.clone();
        move |_: &str, code: Option<i32>| {
            let mut s = shared.lock().unwrap();
            s.exited = Some(code);
            let delivered = send_to_client(&mut s, &Frame::Exited(code));
            drop(s);
            let _ = events.send(Event::Exited);
            if delivered {
                let _ = events.send(Event::Collected);
            }
        }
    };
    if let Err(e) = pty.spawn(
        &config.pane,
        config.cols,
        config.rows,
        config.cwd.as_deref(),
        command,
        Box::new(PassthroughScanner),
        on_data,
        on_exit,
    ) {
        let _ = std::fs::remove_file(&config.socket);
        return Err(io::Error::other(e));
    }

    let stopping = Arc::new(AtomicBool::new(false));
    {
        let pty = pty.clone();
        let shared = Arc::clone(&shared);
        let events = events.clone();
        let stopping = Arc::clone(&stopping);
        let pane = config.pane.clone();
        std::thread::Builder::new()
            .name("hold-accept".into())
            .spawn(move || accept_loop(listener, pty, pane, shared, events, stopping))?;
    }

    // Wait for the child to exit, then for someone to collect it.
    let mut exited = false;
    loop {
        let event = if exited {
            events_rx.recv_timeout(config.exited_grace)
        } else {
            events_rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected)
        };
        match event {
            Ok(Event::Exited) => exited = true,
            Ok(Event::Collected) | Err(_) => break,
        }
    }

    // Stop accepting: flag it, then wake the blocked `accept` with a
    // connection of our own so the thread sees the flag.
    stopping.store(true, Ordering::SeqCst);
    let _ = UnixStream::connect(&config.socket);
    let _ = std::fs::remove_file(&config.socket);
    let code = shared.lock().unwrap().exited.flatten();
    if let Some(client) = shared.lock().unwrap().client.take() {
        let _ = client.stream.shutdown(std::net::Shutdown::Both);
    }
    Ok(code)
}

/// Binds the holder's socket in a directory only this user can enter — the
/// directory, not the socket, is what keeps other users out, so one that
/// exists already must be ours and private. A socket file left by a holder
/// that died is replaced; one with a live holder behind it is an error,
/// never a takeover.
fn bind(path: &Path) -> io::Result<UnixListener> {
    use std::os::unix::fs::MetadataExt;

    // sockaddr_un's path is 104 bytes on macOS, 108 on Linux.
    if path.as_os_str().len() >= 104 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("socket path too long for a Unix socket: {}", path.display()),
        ));
    }
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        let meta = std::fs::symlink_metadata(dir)?;
        let uid = unsafe { libc::geteuid() };
        if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("{} must be a directory owned by you with mode 0700", dir.display()),
            ));
        }
    }
    if path.exists() {
        if UnixStream::connect(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("a holder is already serving {}", path.display()),
            ));
        }
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Writes to the attached client, dropping it if the write fails. Returns
/// whether there was a client and the frame reached it.
fn send_to_client(s: &mut Shared, frame: &Frame) -> bool {
    let Some(client) = s.client.as_mut() else {
        return false;
    };
    if write_frame(&mut client.stream, frame).is_ok() {
        true
    } else {
        s.client = None;
        false
    }
}

fn accept_loop(
    listener: UnixListener,
    pty: PtyManager,
    pane: String,
    shared: Arc<Mutex<Shared>>,
    events: mpsc::Sender<Event>,
    stopping: Arc<AtomicBool>,
) {
    let next_id = AtomicU64::new(1);
    for stream in listener.incoming() {
        if stopping.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = stream else { continue };
        let id = next_id.fetch_add(1, Ordering::Relaxed);
        let (pty, pane, shared, events) =
            (pty.clone(), pane.clone(), Arc::clone(&shared), events.clone());
        let _ = std::thread::Builder::new()
            .name(format!("hold-client-{id}"))
            .spawn(move || {
                let _ = serve_client(id, stream, &pty, &pane, &shared, &events);
            });
    }
}

fn serve_client(
    id: u64,
    stream: UnixStream,
    pty: &PtyManager,
    pane: &str,
    shared: &Mutex<Shared>,
    events: &mpsc::Sender<Event>,
) -> io::Result<()> {
    let mut reader = stream.try_clone()?;
    // A connection that never says hello must not hold a thread forever.
    reader.set_read_timeout(Some(Duration::from_secs(10)))?;
    let Some(Frame::ClientHello { cols, rows, .. }) = read_frame(&mut reader)? else {
        return Ok(());
    };
    reader.set_read_timeout(None)?;
    let _ = pty.resize(pane, cols, rows);

    {
        let mut s = shared.lock().unwrap();
        // One client at a time, and the newest wins: a crashed app's
        // connection must not lock the pane away from its next launch.
        if let Some(old) = s.client.take() {
            let _ = old.stream.shutdown(std::net::Shutdown::Both);
        }
        let mut stream = stream;
        let pid = pty.pids().first().map(|(_, pid)| *pid);
        write_frame(
            &mut stream,
            &Frame::HolderHello {
                version: VERSION,
                pid,
                exit_code: s.exited.flatten(),
            },
        )?;
        write_frame(&mut stream, &Frame::Replay(s.replay.snapshot()))?;
        if let Some(code) = s.exited {
            write_frame(&mut stream, &Frame::Exited(code))?;
            s.client = Some(Client { id, stream });
            drop(s);
            let _ = events.send(Event::Collected);
            return Ok(());
        }
        s.client = Some(Client { id, stream });
    }

    loop {
        match read_frame(&mut reader) {
            Ok(Some(Frame::Input(bytes))) => {
                let _ = pty.write(pane, &bytes);
            }
            Ok(Some(Frame::Resize { cols, rows })) => {
                let _ = pty.resize(pane, cols, rows);
            }
            Ok(Some(Frame::Kill)) => {
                let _ = pty.close(pane);
            }
            Ok(Some(Frame::Detach)) | Ok(None) | Err(_) => break,
            // Holder-to-client frames, or a hello sent twice: ignore.
            Ok(Some(_)) => {}
        }
    }
    let mut s = shared.lock().unwrap();
    if s.client.as_ref().is_some_and(|c| c.id == id) {
        s.client = None;
    }
    Ok(())
}
