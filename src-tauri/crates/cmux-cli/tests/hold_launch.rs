//! The app's path to a held pane, on every platform: `hold::launch` starts
//! the real `mira __hold` from a process that then exits — as the app does
//! when it quits — and `PtyManager::attach_held` picks it up afterwards,
//! twice, as two launches would. On Windows this is the only test that runs
//! the holder as a detached process of its own.

use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cmux_core::hold::conn;
use cmux_core::hold::server::Program;
use cmux_core::osc::PassthroughScanner;
use cmux_core::pty::PtyManager;

const PATIENCE: Duration = Duration::from_secs(if cfg!(windows) { 60 } else { 20 });
/// Set when this binary runs as the short-lived launcher.
const HELPER: &str = "MIRADOR_TEST_LAUNCH_SOCKET";

/// The launcher: start the holder and exit. Does nothing in a normal run.
#[test]
fn launcher() {
    let Ok(socket) = std::env::var(HELPER) else {
        return;
    };
    let script = if cfg!(windows) {
        r#"echo ready; $x = Read-Host; echo "got:$x"; $y = Read-Host"#
    } else {
        "echo ready; read x; echo got:$x; read y"
    };
    cmux_core::hold::launch(
        &PathBuf::from(env!("CARGO_BIN_EXE_mira")),
        "p",
        &PathBuf::from(socket),
        None,
        80,
        24,
        &Program::Command(script.into()),
    )
    .expect("launch");
}

fn location() -> PathBuf {
    let id = format!("mhl-{}", std::process::id());
    if cfg!(windows) {
        PathBuf::from(format!(r"\\.\pipe\{id}"))
    } else {
        let dir = PathBuf::from(format!("/tmp/{id}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }
}

struct Pane {
    out: mpsc::Receiver<Vec<u8>>,
    exit: mpsc::Receiver<Option<i32>>,
    seen: String,
}

/// Attaches the way the app does, answering cursor queries the way its
/// terminal would.
fn attach(mgr: &PtyManager, socket: &std::path::Path) -> Pane {
    let (out_tx, out) = mpsc::channel::<Vec<u8>>();
    let (exit_tx, exit) = mpsc::channel::<Option<i32>>();
    let answer = mgr.clone();
    mgr.attach_held(
        "p",
        socket,
        80,
        24,
        Box::new(PassthroughScanner),
        move |b: &[u8]| {
            if b.windows(4).any(|w| w == b"\x1b[6n") {
                let _ = answer.write("p", b"\x1b[1;1R");
            }
            let _ = out_tx.send(b.to_vec());
        },
        move |_, code| {
            let _ = exit_tx.send(code);
        },
    )
    .expect("attach_held");
    Pane {
        out,
        exit,
        seen: String::new(),
    }
}

impl Pane {
    fn wait_for(&mut self, needle: &str) {
        let deadline = Instant::now() + PATIENCE;
        while !self.seen.contains(needle) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.out.recv_timeout(left) {
                Ok(b) => self.seen.push_str(&String::from_utf8_lossy(&b)),
                Err(_) => panic!("never saw {needle:?}; got {:?}", self.seen),
            }
        }
    }
}

#[test]
fn a_launched_holder_outlives_its_launcher_and_reattaches() {
    if std::env::var(HELPER).is_ok() {
        return; // this run is the launcher, not the test
    }
    let socket = conn::endpoint(&location(), "p");

    // Launch from a process of its own, and let it exit.
    let launcher = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "launcher", "--nocapture"])
        .env(HELPER, &socket)
        .status()
        .unwrap();
    assert!(launcher.success(), "the launcher failed");
    assert!(conn::is_alive(&socket), "the holder did not outlive its launcher");

    // First launch of the "app".
    let first = PtyManager::new();
    let mut pane = attach(&first, &socket);
    pane.wait_for("ready");
    let pid = first.pids().first().map(|(_, p)| *p).expect("held pane reports its pid");
    drop(pane);
    drop(first);

    // Second launch: a fresh manager, the same process, still taking input.
    let second = PtyManager::new();
    let mut pane = attach(&second, &socket);
    assert_eq!(second.pids().first().map(|(_, p)| *p), Some(pid), "not the same process");
    pane.wait_for("ready");
    second.write("p", b"hello\r").unwrap();
    pane.wait_for("got:hello");

    // Closing the pane ends it, and the holder with it.
    second.close("p").unwrap();
    pane.exit.recv_timeout(PATIENCE).expect("no exit after close");
    let deadline = Instant::now() + PATIENCE;
    while conn::is_alive(&socket) {
        assert!(Instant::now() < deadline, "holder still serving after its pane closed");
        std::thread::sleep(Duration::from_millis(50));
    }
}
