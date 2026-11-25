//! `mira __hold-start` and `mira attach` as a person on a remote uses them:
//! a session started detached, then looked at from a terminal of its own.

#![cfg(unix)]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cmux_core::hold::conn;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

const PATIENCE: Duration = Duration::from_secs(10);
const MIRA: &str = env!("CARGO_BIN_EXE_mira");

/// A runtime dir of its own; ends what it holds and goes away after.
struct Runtime(PathBuf);

impl Runtime {
    /// Under /tmp, not `temp_dir()`: macOS's is long enough to push the
    /// holder's socket path past the 104 bytes a Unix socket allows.
    fn new(test: &str) -> Self {
        let dir = PathBuf::from(format!("/tmp/mat-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Runtime(dir)
    }
    fn holders(&self) -> PathBuf {
        self.0.join("mirador").join("holders")
    }
    fn socket(&self, pane: &str) -> PathBuf {
        conn::endpoint(&self.holders(), pane)
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        cmux_core::hold::end_all_in(&self.holders(), PATIENCE);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `mira attach <pane>` in a terminal: what it shows, and keys to type.
struct Attached {
    out: mpsc::Receiver<Vec<u8>>,
    seen: String,
    keys: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

fn attach(runtime: &Path, pane: &str) -> Attached {
    let pty = native_pty_system()
        .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
        .unwrap();
    // Says, after attach, whether it left the terminal as it found it.
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.args([
        "-c",
        r#""$0" attach "$1"; c=$?; stty -a | grep -q -- -icanon && echo "tty: raw" || echo "tty: cooked"; exit $c"#,
        MIRA,
        pane,
    ]);
    cmd.env("XDG_RUNTIME_DIR", runtime);
    let child = pty.slave.spawn_command(cmd).unwrap();
    drop(pty.slave);
    let mut reader = pty.master.try_clone_reader().unwrap();
    let keys = pty.master.take_writer().unwrap();
    let (tx, out) = mpsc::channel();
    std::thread::spawn(move || {
        // Keeps the master alive as long as there is output to read.
        let _master = pty.master;
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    Attached { out, seen: String::new(), keys, child }
}

impl Attached {
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
    fn type_keys(&mut self, keys: &str) {
        self.keys.write_all(keys.as_bytes()).unwrap();
        self.keys.flush().unwrap();
    }
    fn exit_code(&mut self) -> u32 {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.exit_code();
            }
            assert!(Instant::now() < deadline, "attach never exited; showed {:?}", self.seen);
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn a_started_session_is_attached_typed_into_and_ends_with_its_code() {
    let runtime = Runtime::new("start");
    let started = Command::new(MIRA)
        .args(["__hold-start", "--pane", "t1", "--command", "printf hi; cat; exit 7"])
        .env("XDG_RUNTIME_DIR", &runtime.0)
        .output()
        .unwrap();
    assert!(started.status.success(), "{}", String::from_utf8_lossy(&started.stderr));
    assert!(conn::is_alive(&runtime.socket("t1")), "returned before the holder answered");

    let mut term = attach(&runtime.0, "t1");
    term.wait_for("hi");
    term.type_keys("abc");
    term.wait_for("abc");
    // Return, so cat has its line; Ctrl-D on the empty one ends it.
    term.type_keys("\r\x04");
    assert_eq!(term.exit_code(), 7, "showed {:?}", term.seen);
    term.wait_for("tty: cooked");
}

#[test]
fn a_second_attach_takes_the_session_from_the_first() {
    let runtime = Runtime::new("taken");
    let started = Command::new(MIRA)
        .args(["__hold-start", "--pane", "t3", "--command", "printf hi; cat"])
        .env("XDG_RUNTIME_DIR", &runtime.0)
        .output()
        .unwrap();
    assert!(started.status.success(), "{}", String::from_utf8_lossy(&started.stderr));
    let mut first = attach(&runtime.0, "t3");
    first.wait_for("hi");

    let mut second = attach(&runtime.0, "t3");
    second.wait_for("hi"); // the replay
    assert_eq!(first.exit_code(), 1, "showed {:?}", first.seen);
    first.wait_for("mira: session t3 was taken by another attach");
    first.wait_for("tty: cooked");
}

#[test]
fn a_started_session_is_ended_and_its_holder_goes() {
    let runtime = Runtime::new("end");
    let mira = |args: &[&str]| Command::new(MIRA).args(args).env("XDG_RUNTIME_DIR", &runtime.0).output().unwrap();
    let started = mira(&["__hold-start", "--pane", "t2", "--command", "sleep 60"]);
    assert!(started.status.success(), "{}", String::from_utf8_lossy(&started.stderr));
    let socket = runtime.socket("t2");
    let holder = || {
        let out = Command::new("pgrep").args(["-f", &format!("__hold --pane t2 --socket {}", socket.display())]).output().unwrap();
        !out.stdout.is_empty()
    };
    assert!(holder(), "no holder process");

    let ended = mira(&["__hold-end", "--pane", "t2"]);
    assert!(ended.status.success(), "{}", String::from_utf8_lossy(&ended.stderr));
    assert!(!conn::is_alive(&socket), "the holder still answers");
    let deadline = Instant::now() + PATIENCE;
    while holder() {
        assert!(Instant::now() < deadline, "the holder process is still there");
        std::thread::sleep(Duration::from_millis(20));
    }

    // Nothing to end is not an error.
    let nobody = mira(&["__hold-end", "--pane", "nobody"]);
    assert!(nobody.status.success(), "{}", String::from_utf8_lossy(&nobody.stderr));
}

#[test]
fn attaching_to_no_session_says_so() {
    let runtime = Runtime::new("nobody");
    let mut term = attach(&runtime.0, "nobody");
    assert_eq!(term.exit_code(), 1);
    term.wait_for("mira: no session nobody on ");
    term.wait_for(": it has ended, or never started here");
}

#[test]
fn attaching_without_a_terminal_says_so() {
    let out = Command::new(MIRA)
        .args(["attach", "t1"])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stderr), "mira: attach needs a terminal\n");
}
