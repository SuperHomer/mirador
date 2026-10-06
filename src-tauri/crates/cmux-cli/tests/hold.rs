//! `mira __hold` as the app will run it: a real process, started by a
//! parent that goes away, still serving its pane afterwards.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cmux_core::hold::client::attach;
use cmux_core::hold::wire::{read_frame, write_frame, Frame};

const PATIENCE: Duration = Duration::from_secs(20);

fn ps(field: &str, pid: u32) -> Option<String> {
    let out = Command::new("ps")
        .args(["-o", &format!("{field}="), "-p", &pid.to_string()])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

#[test]
fn the_holder_outlives_the_process_that_started_it() {
    let dir = PathBuf::from(format!("/tmp/mhc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let socket = dir.join("p.sock");

    // A parent shell that starts the holder in the background, prints its
    // pid and exits at once — the way the app quitting looks to a holder.
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "'{}' __hold --pane t --socket '{}' --command 'echo held; sleep 30' \
             </dev/null >/dev/null 2>&1 & echo $!",
            env!("CARGO_BIN_EXE_mira"),
            socket.display()
        ))
        .stdout(Stdio::piped())
        .output()
        .unwrap();
    let holder: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();

    let deadline = Instant::now() + PATIENCE;
    while !socket.exists() {
        assert!(Instant::now() < deadline, "holder never bound its socket");
        std::thread::sleep(Duration::from_millis(20));
    }

    // Its own session and process group, not the test's.
    assert_eq!(ps("pgid", holder).as_deref(), Some(holder.to_string().as_str()));
    assert_ne!(ps("pgid", holder), ps("pgid", std::process::id()));

    let mut a = attach(&socket, 80, 24).unwrap();
    a.stream.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    let mut seen = String::from_utf8_lossy(&a.replay).into_owned();
    let deadline = Instant::now() + PATIENCE;
    while !seen.contains("held") {
        assert!(Instant::now() < deadline, "no output; got {seen:?}");
        if let Ok(Some(Frame::Output(b))) = read_frame(&mut a.stream) {
            seen.push_str(&String::from_utf8_lossy(&b));
        }
    }

    // Ending it ends the process too.
    write_frame(&mut a.stream, &Frame::Kill).unwrap();
    let deadline = Instant::now() + PATIENCE;
    while ps("pid", holder).is_some() {
        assert!(Instant::now() < deadline, "holder still running after Kill");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!socket.exists(), "socket left behind");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The app's side of a held pane, end to end: `hold::launch` starts the real
/// holder, `PtyManager::attach_held` drives it through the same scanner,
/// sink and exit hook a local pane uses, and a second manager — the next
/// launch — reattaches to the same process.
#[test]
fn a_held_pane_works_like_a_local_one_and_survives_its_manager() {
    use cmux_core::hold::server::Program;
    use cmux_core::osc::{OscEvent, OscScanner};
    use cmux_core::pty::PtyManager;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};

    let dir = PathBuf::from(format!("/tmp/mhp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let socket = dir.join("p.sock");
    let marker = dir.join("printed");
    let mira = PathBuf::from(env!("CARGO_BIN_EXE_mira"));
    // A failed assertion must not leave a holder running for good.
    struct Reap(PathBuf);
    impl Drop for Reap {
        fn drop(&mut self) {
            let _ = Command::new("pkill")
                .args(["-f", &format!("--socket {}", self.0.display())])
                .status();
            let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
        }
    }
    let _reap = Reap(socket.clone());

    // A notification goes out before anyone attaches — the marker says it
    // has, since a login shell's startup time is anyone's guess — then the
    // shell waits.
    let script = format!(
        "printf '\\033]777;notify;T;early\\007'; echo ready; touch {}; \
         read line; echo got:$line; stty size; read never",
        marker.display()
    );
    cmux_core::hold::launch(&mira, "p", &socket, None, 80, 24, &Program::Command(script))
        .unwrap();
    let deadline = Instant::now() + PATIENCE;
    while !marker.exists() {
        assert!(Instant::now() < deadline, "the script never ran");
        std::thread::sleep(Duration::from_millis(20));
    }

    let attach = |mgr: &PtyManager, events: Arc<Mutex<Vec<OscEvent>>>| {
        let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
        let (exit_tx, exit_rx) = mpsc::channel::<Option<i32>>();
        mgr.attach_held(
            "p",
            &socket,
            80,
            24,
            Box::new(OscScanner::new(move |e| events.lock().unwrap().push(e))),
            move |b: &[u8]| {
                let _ = out_tx.send(b.to_vec());
            },
            move |_, code| {
                let _ = exit_tx.send(code);
            },
        )
        .unwrap();
        (out_rx, exit_rx)
    };
    let wait_for = |rx: &mpsc::Receiver<Vec<u8>>, seen: &mut String, needle: &str| {
        let deadline = Instant::now() + PATIENCE;
        while !seen.contains(needle) {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(b) => seen.push_str(&String::from_utf8_lossy(&b)),
                Err(_) => panic!("never saw {needle:?}; got {seen:?}"),
            }
        }
    };

    // First launch: the replay carries the early notification, which must
    // not fire again — but it is still stripped from what the pane shows.
    let first = PtyManager::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let (out, _exit) = attach(&first, Arc::clone(&events));
    let mut seen = String::new();
    wait_for(&out, &mut seen, "ready");
    assert!(!seen.contains("early"), "the OSC sequence reached the screen: {seen:?}");
    assert!(
        !events.lock().unwrap().iter().any(|e| matches!(e, OscEvent::Notification { .. })),
        "a replayed notification fired"
    );
    let pid = first.pids().first().map(|(_, pid)| *pid).expect("held pane reports its pid");

    // The app quits: its manager goes away without killing anything.
    drop(out);
    drop(first);

    // Next launch: a fresh manager reattaches to the same process.
    let second = PtyManager::new();
    let (out, exit) = attach(&second, Arc::new(Mutex::new(Vec::new())));
    assert_eq!(second.pids().first().map(|(_, p)| *p), Some(pid), "not the same process");
    let mut seen = String::new();
    wait_for(&out, &mut seen, "ready");
    second.resize("p", 100, 30).unwrap();
    second.write("p", b"hello\r").unwrap();
    wait_for(&out, &mut seen, "got:hello");
    wait_for(&out, &mut seen, "30 100");

    // Closing the pane ends it, wherever it runs.
    second.close("p").unwrap();
    exit.recv_timeout(PATIENCE).expect("no exit after close");
    let deadline = Instant::now() + PATIENCE;
    while ps("pid", pid).is_some() {
        assert!(Instant::now() < deadline, "child {pid} survived close");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A holder must not keep descriptors it inherited: one that held on to
/// the app's connection to another holder would keep that connection open
/// after the app quits, and freeze that holder's child behind a socket
/// nobody reads.
#[test]
fn a_holder_keeps_nothing_it_inherited() {
    use std::os::fd::AsRawFd;
    use std::os::unix::net::{UnixListener, UnixStream};

    let dir = PathBuf::from(format!("/tmp/mhi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let canary = dir.join("canary.sock");
    let socket = dir.join("h").join("p.sock");

    // An inheritable listener: close-on-exec cleared, as in the race.
    let listener = UnixListener::bind(&canary).unwrap();
    unsafe {
        libc::fcntl(listener.as_raw_fd(), libc::F_SETFD, 0);
    }
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "'{}' __hold --pane t --socket '{}' --command 'sleep 30' \
             </dev/null >/dev/null 2>&1 & echo $!",
            env!("CARGO_BIN_EXE_mira"),
            socket.display()
        ))
        .output()
        .unwrap();
    let holder: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
    let deadline = Instant::now() + PATIENCE;
    while !socket.exists() {
        assert!(Instant::now() < deadline, "holder never bound its socket");
        std::thread::sleep(Duration::from_millis(20));
    }

    // Ours gone, only an inheritor could still be listening. Other tests in
    // this binary fork short-lived children (`sh`, `ps`) that can inherit it
    // for a moment too, so wait those out: only the holder lives on.
    drop(listener);
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut leaked = true;
    while Instant::now() < deadline {
        if UnixStream::connect(&canary).is_err() {
            leaked = false;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let mut a = attach(&socket, 80, 24).unwrap();
    write_frame(&mut a.stream, &Frame::Kill).unwrap();
    let deadline = Instant::now() + PATIENCE;
    while ps("pid", holder).is_some() {
        assert!(Instant::now() < deadline, "holder still running after Kill");
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!leaked, "the holder kept a descriptor it inherited");
}
