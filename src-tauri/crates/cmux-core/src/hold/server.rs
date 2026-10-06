//! The holder: one pane's PTY and child, kept alive in a process of its own
//! so they outlive the app, and served to whichever client attaches.
//!
//! It is a `PtyManager` with one pane, a [`ReplayBuffer`] of its output and
//! a Unix socket. Output goes into the buffer always, and to the attached
//! client when there is one. A client that attaches gets a `HolderHello`,
//! then everything in the buffer as one `Replay`, then live `Output` — all
//! under one lock, so nothing printed in between is lost or sent twice.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::osc::{OscEvent, OscScanner, PassthroughScanner, StreamScanner};
use crate::pty::PtyManager;

use super::conn::{self, Conn, Listener};
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
    stream: Conn,
}

struct Shared {
    replay: ReplayBuffer,
    client: Option<Client>,
    /// `Some` once the child has exited, holding its code.
    exited: Option<Option<i32>>,
    /// Sees every chunk, so its parse state survives attaches; it only
    /// counts while nobody is attached (see `Missed`).
    detector: Box<dyn StreamScanner>,
    /// Whether the detector should count: true while detached.
    counting: Arc<AtomicBool>,
    missed: Arc<Mutex<Missed>>,
}

/// Notifications raised while no client was attached: reported in the next
/// `HolderHello`, then reset.
#[derive(Default)]
struct Missed {
    count: u32,
    last: Option<String>,
}

fn missed_detector(counting: Arc<AtomicBool>, missed: Arc<Mutex<Missed>>) -> Box<dyn StreamScanner> {
    Box::new(OscScanner::new(move |event| {
        if let OscEvent::Notification { title, body } = event {
            if counting.load(Ordering::SeqCst) {
                let mut m = missed.lock().unwrap();
                m.count = m.count.saturating_add(1);
                m.last = Some(match title {
                    Some(title) if !title.is_empty() => format!("{title}: {body}"),
                    _ => body,
                });
            }
        }
    }))
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
    let listener = conn::bind(&config.socket)?;
    let pty = PtyManager::unthrottled();
    let counting = Arc::new(AtomicBool::new(true));
    let missed = Arc::new(Mutex::new(Missed::default()));
    let shared = Arc::new(Mutex::new(Shared {
        replay: ReplayBuffer::new(config.replay_bytes),
        client: None,
        exited: None,
        detector: missed_detector(Arc::clone(&counting), Arc::clone(&missed)),
        counting,
        missed,
    }));
    let (events, events_rx) = mpsc::channel::<Event>();

    let command = match &config.program {
        Program::Shell => None,
        Program::Command(cmd) => Some(crate::pty::shell::run_command(cmd, config.cwd.as_deref())),
        Program::Ssh(host) => Some(crate::ssh::interactive_command(host)),
    };
    let on_data = {
        let shared = Arc::clone(&shared);
        let pty = pty.clone();
        let pane = config.pane.clone();
        move |bytes: &[u8]| {
            let mut s = shared.lock().unwrap();
            s.replay.push(bytes);
            // With nobody attached there is no terminal to answer "where is
            // the cursor?", and Windows' ConPTY asks it before it starts
            // the shell at all: an unattached holder would hang forever.
            // Answer as a terminal at the top-left would; a client that
            // attaches redraws anyway.
            if s.client.is_none() && contains(bytes, b"\x1b[6n") {
                answer_cursor_query(&pty, &pane);
            }
            let _ = s.detector.scan(bytes);
            send_to_client(&mut s, &Frame::Output(bytes.to_vec()));
            s.counting.store(s.client.is_none(), Ordering::SeqCst);
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
        #[cfg(unix)]
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
    let _ = conn::connect(&config.socket);
    // A socket file outlives its listener; a pipe does not.
    #[cfg(unix)]
    let _ = std::fs::remove_file(&config.socket);
    let code = shared.lock().unwrap().exited.flatten();
    if let Some(client) = shared.lock().unwrap().client.take() {
        client.stream.shutdown();
    }
    Ok(code)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Answers a cursor-position query on the child's input. The query can be
/// the child's very first output, before the pane is in the manager's map,
/// so the write is retried briefly off this thread.
fn answer_cursor_query(pty: &PtyManager, pane: &str) {
    let (pty, pane) = (pty.clone(), pane.to_string());
    std::thread::spawn(move || {
        for _ in 0..200 {
            if pty.write(&pane, b"\x1b[1;1R").is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
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
    listener: Listener,
    pty: PtyManager,
    pane: String,
    shared: Arc<Mutex<Shared>>,
    events: mpsc::Sender<Event>,
    stopping: Arc<AtomicBool>,
) {
    let next_id = AtomicU64::new(1);
    loop {
        let stream = listener.accept();
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
    stream: Conn,
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
            old.stream.shutdown();
        }
        let mut stream = stream;
        let pid = pty.pids().first().map(|(_, pid)| *pid);
        let missed = std::mem::take(&mut *s.missed.lock().unwrap());
        write_frame(
            &mut stream,
            &Frame::HolderHello {
                version: VERSION,
                pid,
                exit_code: s.exited.flatten(),
                missed: missed.count,
                last_missed: missed.last,
            },
        )?;
        write_frame(&mut stream, &Frame::Replay(s.replay.snapshot()))?;
        s.counting.store(false, Ordering::SeqCst);
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
        s.counting.store(true, Ordering::SeqCst);
    }
    Ok(())
}
