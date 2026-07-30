use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Condvar, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::osc::StreamScanner;

pub mod shell;

pub type PaneId = String;

/// Every pane's processes inherit this: it is how `mira` (and therefore an
/// agent's hooks) knows which pane it is running in, on every platform.
/// The tty-based lookup remains as a fallback for processes that lose the
/// environment.
pub const PANE_ENV: &str = "MIRA_PANE";

/// Pause reading the PTY when this many bytes are in flight to the UI…
const HIGH_WATERMARK: u64 = 2 * 1024 * 1024;
/// …and resume once the UI has acked back down to this level.
const LOW_WATERMARK: u64 = 512 * 1024;

#[derive(Default)]
struct FlowState {
    unacked: u64,
    closed: bool,
}

/// Byte-accounting backpressure between the PTY reader thread and the UI.
/// Without it, a fast producer (`yes`, `cat bigfile`) overruns the webview.
/// When the reader pauses, the kernel pipe buffer fills and blocks the child
/// process — the same flow control a native terminal gets.
#[derive(Default)]
struct FlowControl {
    state: Mutex<FlowState>,
    cond: Condvar,
}

impl FlowControl {
    /// Called by the reader thread after forwarding `n` bytes; blocks while
    /// the high watermark is exceeded. Returns false if the pane was closed.
    fn add_and_wait(&self, n: u64) -> bool {
        let mut s = self.state.lock().unwrap();
        s.unacked += n;
        while s.unacked >= HIGH_WATERMARK && !s.closed {
            s = self.cond.wait(s).unwrap();
        }
        !s.closed
    }

    fn ack(&self, n: u64) {
        let mut s = self.state.lock().unwrap();
        s.unacked = s.unacked.saturating_sub(n);
        if s.unacked <= LOW_WATERMARK {
            self.cond.notify_all();
        }
    }

    fn close(&self) {
        self.state.lock().unwrap().closed = true;
        self.cond.notify_all();
    }
}

type Sink = Arc<Mutex<Box<dyn FnMut(&[u8]) + Send>>>;

struct PtyPane {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    flow: Arc<FlowControl>,
    /// Replaceable output callback: a remounted frontend pane re-attaches
    /// with a fresh channel without respawning the shell.
    sink: Sink,
    pid: Option<u32>,
}

#[derive(Default)]
struct Inner {
    panes: Mutex<HashMap<PaneId, PtyPane>>,
}

/// Owns every live PTY. Reader threads stream output through the `on_data`
/// callback supplied at spawn time, so this crate stays free of any IPC or
/// Tauri dependency.
#[derive(Default, Clone)]
pub struct PtyManager {
    inner: Arc<Inner>,
}

impl PtyManager {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        &self,
        id: &str,
        cols: u16,
        rows: u16,
        cwd: Option<&str>,
        command: Option<CommandBuilder>,
        mut scanner: Box<dyn StreamScanner>,
        on_data: impl FnMut(&[u8]) + Send + 'static,
        on_exit: impl FnOnce(&str, Option<i32>) + Send + 'static,
    ) -> Result<(), String> {
        if self.inner.panes.lock().unwrap().contains_key(id) {
            return Err(format!("pane {id} already running"));
        }
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;

        let mut command = command.unwrap_or_else(|| shell::interactive(cwd));
        command.env(PANE_ENV, id);
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| e.to_string())?;
        // Close our copy of the slave end so reads hit EOF when the child exits.
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let id = id.to_string();
        let pid = child.process_id();
        let flow = Arc::new(FlowControl::default());
        let sink: Sink = Arc::new(Mutex::new(Box::new(on_data)));

        {
            let inner = Arc::clone(&self.inner);
            let flow = Arc::clone(&flow);
            let sink = Arc::clone(&sink);
            let id = id.clone();

            // Reader → forwarder handoff. Bounded so a paused forwarder
            // backpressures the reader, which in turn lets the kernel tty
            // buffer fill and block the child — end-to-end flow control.
            let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(64);

            std::thread::Builder::new()
                .name(format!("pty-reader-{id}"))
                .spawn(move || {
                    let mut buf = [0u8; 8192];
                    loop {
                        match reader.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                if tx.send(buf[..n].to_vec()).is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    // Dropping tx disconnects the forwarder, which owns cleanup.
                })
                .map_err(|e| e.to_string())?;

            std::thread::Builder::new()
                .name(format!("pty-forward-{id}"))
                .spawn(move || {
                    // Coalesce whatever the reader has produced into one IPC
                    // message (tty queues yield tiny chunks; per-message
                    // webview overhead dominates throughput otherwise).
                    const MAX_BATCH: usize = 128 * 1024;
                    // After close() we keep draining so the dying child's
                    // blocked tty writes can flush and the reader sees EOF.
                    let mut forwarding = true;
                    while let Ok(first) = rx.recv() {
                        let mut batch = first;
                        while batch.len() < MAX_BATCH {
                            match rx.try_recv() {
                                Ok(more) => batch.extend_from_slice(&more),
                                Err(_) => break,
                            }
                        }
                        if !forwarding {
                            continue;
                        }
                        let n = batch.len() as u64;
                        let out = scanner.scan(&batch);
                        if !out.is_empty() {
                            (sink.lock().unwrap())(&out);
                        }
                        if !flow.add_and_wait(n) {
                            forwarding = false;
                        }
                    }
                    // Single cleanup path: remove the pane and reap the child
                    // whether the process exited on its own or close() killed it.
                    let pane = inner.panes.lock().unwrap().remove(&id);
                    let exit_code = pane.and_then(|mut pane| {
                        pane.child.wait().ok().map(|s| s.exit_code() as i32)
                    });
                    on_exit(&id, exit_code);
                })
                .map_err(|e| e.to_string())?;
        }

        self.inner.panes.lock().unwrap().insert(
            id,
            PtyPane {
                writer,
                master: pair.master,
                child,
                flow,
                sink,
                pid,
            },
        );
        Ok(())
    }

    /// Redirects a running pane's output to a new callback (frontend
    /// remount). Returns false if the pane is not running.
    pub fn set_sink(&self, id: &str, on_data: impl FnMut(&[u8]) + Send + 'static) -> bool {
        if let Some(pane) = self.inner.panes.lock().unwrap().get(id) {
            *pane.sink.lock().unwrap() = Box::new(on_data);
            true
        } else {
            false
        }
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.inner.panes.lock().unwrap().contains_key(id)
    }

    /// (pane_id, shell pid) for every live pane.
    pub fn pids(&self) -> Vec<(String, u32)> {
        self.inner
            .panes
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(id, p)| p.pid.map(|pid| (id.clone(), pid)))
            .collect()
    }

    pub fn write(&self, id: &str, data: &[u8]) -> Result<(), String> {
        let mut panes = self.inner.panes.lock().unwrap();
        let pane = panes.get_mut(id).ok_or_else(|| format!("no pane {id}"))?;
        pane.writer.write_all(data).map_err(|e| e.to_string())
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let panes = self.inner.panes.lock().unwrap();
        let pane = panes.get(id).ok_or_else(|| format!("no pane {id}"))?;
        pane.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())
    }

    /// Acknowledge `n` bytes processed by the UI, potentially resuming a
    /// paused reader. Unknown panes are ignored (acks race pane exit).
    pub fn ack(&self, id: &str, n: u64) {
        if let Some(pane) = self.inner.panes.lock().unwrap().get(id) {
            pane.flow.ack(n);
        }
    }

    /// Kill the pane's process tree. Removal and reaping happen in the
    /// reader thread's cleanup path once it sees EOF.
    pub fn close(&self, id: &str) -> Result<(), String> {
        let mut panes = self.inner.panes.lock().unwrap();
        if let Some(pane) = panes.get_mut(id) {
            // Unpark a flow-control-paused reader; it switches to drain mode
            // so the child's pending tty writes can flush and EOF arrives.
            pane.flow.close();
            let pid = pane.child.process_id();
            signal_group(pid, Signal::Hangup);
            #[cfg(windows)]
            {
                // No process groups: taskkill /T walks the tree so a shell's
                // children (the dev server, the test runner) go with it.
                kill_tree(pid);
                let _ = pane.child.kill();
            }

            // Escalate to SIGKILL if the process ignores SIGHUP. The pane
            // still being in the map guarantees the pid hasn't been reaped,
            // so it can't have been reused by another process.
            #[cfg(unix)]
            {
                let inner = Arc::clone(&self.inner);
                let id = id.to_string();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                    if inner.panes.lock().unwrap().contains_key(&id) {
                        signal_group(pid, Signal::Kill);
                    }
                });
            }
        }
        Ok(())
    }
}

/// `Kill` is only used by the unix escalation path.
#[cfg_attr(not(unix), allow(dead_code))]
enum Signal {
    Hangup,
    Kill,
}

/// Signal the child's whole process group (it is a session leader, so its
/// pgid == pid). Catches pipeline children like `yes` under `sh -c`.
#[cfg(unix)]
fn signal_group(pid: Option<u32>, signal: Signal) {
    if let Some(pid) = pid {
        let sig = match signal {
            Signal::Hangup => libc::SIGHUP,
            Signal::Kill => libc::SIGKILL,
        };
        unsafe {
            libc::killpg(pid as i32, sig);
        }
    }
}

#[cfg(not(unix))]
fn signal_group(_pid: Option<u32>, _signal: Signal) {}

/// Kills a process and everything it spawned. `taskkill` is the supported
/// way to do this without holding a job object per pane.
#[cfg(windows)]
fn kill_tree(pid: Option<u32>) {
    if let Some(pid) = pid {
        let _ = crate::proc::command("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
    }
}

/// PTY behavior is exercised with POSIX one-liners (`yes`, `dd`); the
/// ConPTY path is covered by running the app on Windows.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::osc::PassthroughScanner;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    fn shell_cmd(script: &str) -> CommandBuilder {
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.arg("-c");
        cmd.arg(script);
        cmd
    }

    #[test]
    fn spawn_streams_output_and_exits() {
        let mgr = PtyManager::new();
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let (exit_tx, exit_rx) = mpsc::channel::<()>();

        let id = "pane-basic".to_string();
        mgr.spawn(
                &id,
                80,
                24,
                None,
                Some(shell_cmd("printf hello-cmux")),
                Box::new(PassthroughScanner),
                move |bytes| {
                    let _ = tx.send(bytes.to_vec());
                },
                move |_, _| {
                    let _ = exit_tx.send(());
                },
            )
            .unwrap();

        exit_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("child should exit");

        let mut all = Vec::new();
        while let Ok(chunk) = rx.try_recv() {
            all.extend(chunk);
        }
        let text = String::from_utf8_lossy(&all);
        assert!(text.contains("hello-cmux"), "got: {text:?}");

        // Cleanup path removed the pane: writes now fail.
        assert!(mgr.write(&id, b"x").is_err());
    }

    #[test]
    fn flow_control_pauses_fast_producer_until_acked() {
        let mgr = PtyManager::new();
        let received = Arc::new(AtomicU64::new(0));
        let (exit_tx, exit_rx) = mpsc::channel::<()>();

        const BURST: u64 = 32 * 1024 * 1024;

        let recv = Arc::clone(&received);
        let id = "pane-flow".to_string();
        mgr.spawn(
                &id,
                80,
                24,
                None,
                // 32MB of NULs in 64KB blocks: fast, and no LF→CRLF expansion
                Some(shell_cmd("dd if=/dev/zero bs=65536 count=512 2>/dev/null")),
                Box::new(PassthroughScanner),
                move |bytes| {
                    recv.fetch_add(bytes.len() as u64, Ordering::SeqCst);
                },
                move |_, _| {
                    let _ = exit_tx.send(());
                },
            )
            .unwrap();

        // Without acks the reader must stall right at the high watermark.
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while received.load(Ordering::SeqCst) < HIGH_WATERMARK
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        let stalled = received.load(Ordering::SeqCst);
        assert!(
            stalled >= HIGH_WATERMARK,
            "producer never reached the watermark, got {stalled}"
        );
        // Overshoot is bounded by one coalesced batch (128KB max).
        assert!(
            stalled < HIGH_WATERMARK + 128 * 1024,
            "reader overshot the high watermark, got {stalled}"
        );
        std::thread::sleep(Duration::from_millis(500));
        assert_eq!(
            received.load(Ordering::SeqCst),
            stalled,
            "reader must stay paused without acks"
        );

        // Ack everything repeatedly: the producer drains to completion.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            mgr.ack(&id, HIGH_WATERMARK);
            match exit_rx.recv_timeout(Duration::from_millis(20)) {
                Ok(()) => break,
                Err(_) if std::time::Instant::now() < deadline => continue,
                Err(e) => panic!("producer never finished after acks: {e}"),
            }
        }
        assert!(
            received.load(Ordering::SeqCst) >= BURST,
            "should receive the full burst, got {}",
            received.load(Ordering::SeqCst)
        );
    }

    #[test]
    fn close_unblocks_paused_reader() {
        let mgr = PtyManager::new();
        let (exit_tx, exit_rx) = mpsc::channel::<()>();

        let id = "pane-close".to_string();
        mgr.spawn(
                &id,
                80,
                24,
                None,
                Some(shell_cmd("yes")),
                Box::new(PassthroughScanner),
                |_| {},
                move |_, _| {
                    let _ = exit_tx.send(());
                },
            )
            .unwrap();

        // Let it hit the watermark and park, then close.
        std::thread::sleep(Duration::from_millis(500));
        mgr.close(&id).unwrap();
        exit_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("close must unblock the paused reader thread");
    }
}
