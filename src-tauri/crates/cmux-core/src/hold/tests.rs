//! A real holder around a real shell, driven over its socket. These are
//! the first tests in this repo that run a PTY end to end.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::client::{attach, Attached};
use super::server::{serve, HoldConfig, Program};
use super::wire::{read_frame, write_frame, Frame};

const PATIENCE: Duration = Duration::from_secs(20);

/// A private directory per test: short (socket paths are capped at ~104
/// bytes) and 0700, which the holder insists on.
fn socket() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = PathBuf::from(format!(
        "/tmp/mh-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("p.sock")
}

fn start(command: &str, grace: Duration) -> (PathBuf, JoinHandle<io::Result<Option<i32>>>) {
    let path = socket();
    let config = HoldConfig {
        pane: "test-pane".into(),
        socket: path.clone(),
        cwd: None,
        cols: 80,
        rows: 24,
        program: Program::Command(command.into()),
        replay_bytes: HoldConfig::DEFAULT_REPLAY_BYTES,
        exited_grace: grace,
    };
    let holder = std::thread::spawn(move || serve(config));
    let deadline = Instant::now() + PATIENCE;
    while !path.exists() {
        assert!(Instant::now() < deadline, "holder never bound {}", path.display());
        std::thread::sleep(Duration::from_millis(10));
    }
    (path, holder)
}

fn connect(path: &std::path::Path) -> Attached {
    let a = attach(path, 80, 24).expect("attach");
    a.stream.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    a
}

/// Reads `Output` until `needle` has appeared (searching `seen` too), or
/// until `Exited`/EOF. Returns everything seen and the exit, if any.
fn read_until(a: &mut Attached, mut seen: Vec<u8>, needle: &str) -> (String, Option<Option<i32>>) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        if String::from_utf8_lossy(&seen).contains(needle) {
            return (String::from_utf8_lossy(&seen).into_owned(), None);
        }
        assert!(
            Instant::now() < deadline,
            "never saw {needle:?}; got {:?}",
            String::from_utf8_lossy(&seen)
        );
        match read_frame(&mut a.stream) {
            Ok(Some(Frame::Output(bytes))) => seen.extend(bytes),
            Ok(Some(Frame::Exited(code))) => {
                return (String::from_utf8_lossy(&seen).into_owned(), Some(code))
            }
            Ok(None) => return (String::from_utf8_lossy(&seen).into_owned(), None),
            Ok(Some(_)) => {}
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(e) => panic!("read: {e}"),
        }
    }
}

fn wait_exit(a: &mut Attached) -> Option<i32> {
    let deadline = Instant::now() + PATIENCE;
    loop {
        assert!(Instant::now() < deadline, "no Exited frame");
        match read_frame(&mut a.stream) {
            Ok(Some(Frame::Exited(code))) => return code,
            Ok(None) => panic!("EOF before Exited"),
            _ => {}
        }
    }
}

fn send(a: &mut Attached, frame: Frame) {
    write_frame(&mut a.stream, &frame).unwrap();
}

#[test]
fn output_input_and_exit_reach_the_client() {
    let (path, holder) = start("echo hello; read x; echo got:$x", PATIENCE);
    let mut a = connect(&path);
    assert!(a.pid.is_some());
    assert_eq!(a.exit_code, None);
    let replay = std::mem::take(&mut a.replay);
    read_until(&mut a, replay, "hello");

    send(&mut a, Frame::Input(b"abc\r".to_vec()));
    let (seen, _) = read_until(&mut a, Vec::new(), "got:abc");
    assert!(seen.contains("got:abc"));
    assert_eq!(wait_exit(&mut a), Some(0));

    assert_eq!(holder.join().unwrap().unwrap(), Some(0));
    assert!(!path.exists(), "socket left behind");
}

#[test]
fn reattach_replays_what_was_printed_while_detached() {
    let (path, holder) = start("echo one; sleep 1; echo two; sleep 30", PATIENCE);
    let mut a = connect(&path);
    let replay = std::mem::take(&mut a.replay);
    read_until(&mut a, replay, "one");
    send(&mut a, Frame::Detach);
    drop(a);

    std::thread::sleep(Duration::from_millis(1500));
    let mut b = connect(&path);
    let replay = String::from_utf8_lossy(&b.replay).into_owned();
    assert!(replay.contains("one") && replay.contains("two"), "replay: {replay:?}");

    send(&mut b, Frame::Kill);
    wait_exit(&mut b);
    holder.join().unwrap().unwrap();
}

#[test]
fn an_exit_while_detached_waits_for_a_client_to_collect_it() {
    // The marker is the child's last act, so once it exists the exit is at
    // most a moment away — no guessing how long a login shell takes.
    let marker = std::env::temp_dir().join(format!("mh-done-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let (path, holder) = start(&format!("echo bye; touch {}", marker.display()), PATIENCE);
    let deadline = Instant::now() + PATIENCE;
    while !marker.exists() {
        assert!(Instant::now() < deadline, "child never finished");
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(500));
    // Nobody attached when it exited, and the holder is still waiting.
    assert!(!holder.is_finished(), "holder quit before anyone saw the exit");
    let _ = std::fs::remove_file(&marker);

    let mut a = connect(&path);
    assert_eq!(a.exit_code, Some(0));
    assert!(String::from_utf8_lossy(&a.replay).contains("bye"));
    assert_eq!(wait_exit(&mut a), Some(0));
    assert_eq!(holder.join().unwrap().unwrap(), Some(0));
}

#[test]
fn an_uncollected_exit_gives_up_after_the_grace_period() {
    let (_path, holder) = start("true", Duration::from_millis(300));
    let deadline = Instant::now() + PATIENCE;
    while !holder.is_finished() {
        assert!(Instant::now() < deadline, "holder outlived its grace period");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(holder.join().unwrap().unwrap(), Some(0));
}

#[test]
fn a_new_client_displaces_the_old_one() {
    let (path, holder) = start("echo ready; sleep 30", PATIENCE);
    let mut first = connect(&path);
    let replay = std::mem::take(&mut first.replay);
    read_until(&mut first, replay, "ready");

    let mut second = connect(&path);
    let deadline = Instant::now() + PATIENCE;
    loop {
        assert!(Instant::now() < deadline, "first client never cut off");
        // The holder shuts the old connection down: EOF, or a reset.
        match read_frame(&mut first.stream) {
            Ok(None) => break,
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {}
            Err(_) => break,
            Ok(Some(_)) => {}
        }
    }

    send(&mut second, Frame::Kill);
    wait_exit(&mut second);
    holder.join().unwrap().unwrap();
}

#[test]
fn the_attach_size_reaches_the_child() {
    let (path, holder) = start("sleep 1; stty size; sleep 30", PATIENCE);
    let mut a = attach(&path, 100, 30).unwrap();
    a.stream.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    let replay = std::mem::take(&mut a.replay);
    read_until(&mut a, replay, "30 100");

    send(&mut a, Frame::Kill);
    wait_exit(&mut a);
    holder.join().unwrap().unwrap();
}

#[test]
fn a_live_holder_is_never_taken_over() {
    let (path, holder) = start("sleep 30", PATIENCE);
    let second = HoldConfig {
        pane: "intruder".into(),
        socket: path.clone(),
        cwd: None,
        cols: 80,
        rows: 24,
        program: Program::Command("true".into()),
        replay_bytes: 1024,
        exited_grace: PATIENCE,
    };
    let err = serve(second).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::AddrInUse);

    let mut a = connect(&path);
    send(&mut a, Frame::Kill);
    wait_exit(&mut a);
    holder.join().unwrap().unwrap();
}

/// Leaves a socket file nobody listens on. Not just bind-then-drop: on
/// macOS a socket is made close-on-exec a moment after it is created, so a
/// child that another test forks in that moment inherits it and keeps it
/// answering until the child exits. Wait that out.
fn stale_socket(path: &std::path::Path) {
    drop(std::os::unix::net::UnixListener::bind(path).unwrap());
    let deadline = Instant::now() + PATIENCE;
    while UnixStream::connect(path).is_ok() {
        assert!(Instant::now() < deadline, "{} never went stale", path.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_stale_socket_file_is_replaced() {
    let path = socket();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path.parent().unwrap(), std::fs::Permissions::from_mode(0o700))
        .unwrap();
    stale_socket(&path);
    assert!(path.exists());

    let config = HoldConfig {
        pane: "p".into(),
        socket: path.clone(),
        cwd: None,
        cols: 80,
        rows: 24,
        program: Program::Command("echo fresh".into()),
        replay_bytes: 1024,
        exited_grace: Duration::from_millis(200),
    };
    assert_eq!(serve(config).unwrap(), Some(0));
}

#[test]
fn a_directory_others_can_enter_is_refused() {
    let path = socket();
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let config = HoldConfig {
        pane: "p".into(),
        socket: path,
        cwd: None,
        cols: 80,
        rows: 24,
        program: Program::Command("true".into()),
        replay_bytes: 1024,
        exited_grace: Duration::from_millis(200),
    };
    assert_eq!(serve(config).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn notifications_raised_while_detached_are_reported_once() {
    // A marker, not polling: every attach counts as being seen live, so
    // checking by attaching could swallow the very notification under test.
    let marker = std::env::temp_dir().join(format!("mh-sent-{}", std::process::id()));
    let _ = std::fs::remove_file(&marker);
    let (path, holder) = start(
        &format!(
            "echo up; read a; printf '\\033]777;notify;Claude Code;finished responding\\007'; \
             touch {}; read b; printf '\\033]9;seen live\\007'; echo live; read c",
            marker.display()
        ),
        PATIENCE,
    );
    let mut a = connect(&path);
    assert_eq!(a.missed, 0);
    let replay = std::mem::take(&mut a.replay);
    read_until(&mut a, replay, "up");
    // Leave, then let the child notify with nobody attached.
    send(&mut a, Frame::Input(b"go\r".to_vec()));
    send(&mut a, Frame::Detach);
    drop(a);
    let deadline = Instant::now() + PATIENCE;
    while !marker.exists() {
        assert!(Instant::now() < deadline, "the notification was never sent");
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = std::fs::remove_file(&marker);
    let mut b = connect(&path);
    assert_eq!(b.missed, 1);
    assert_eq!(b.last_missed.as_deref(), Some("Claude Code: finished responding"));

    // One raised while attached was seen live: not counted for next time.
    send(&mut b, Frame::Input(b"go\r".to_vec()));
    read_until(&mut b, Vec::new(), "live");
    send(&mut b, Frame::Detach);
    drop(b);
    let mut c = connect(&path);
    assert_eq!((c.missed, c.last_missed.as_deref()), (0, None));

    send(&mut c, Frame::Kill);
    wait_exit(&mut c);
    holder.join().unwrap().unwrap();
}

#[test]
fn end_all_ends_every_holder_and_sweeps_dead_sockets() {
    let dir = socket().parent().unwrap().to_path_buf();
    let mut holders = Vec::new();
    for pane in ["a", "b"] {
        let config = HoldConfig {
            pane: pane.into(),
            socket: dir.join(format!("{pane}.sock")),
            cwd: None,
            cols: 80,
            rows: 24,
            program: Program::Command("sleep 60".into()),
            replay_bytes: 1024,
            exited_grace: PATIENCE,
        };
        holders.push(std::thread::spawn(move || serve(config)));
    }
    let deadline = Instant::now() + PATIENCE;
    while super::live_panes_in(&dir).len() < 2 {
        assert!(Instant::now() < deadline, "holders never came up");
        std::thread::sleep(Duration::from_millis(20));
    }
    // A socket file whose holder died long ago.
    stale_socket(&dir.join("dead.sock"));
    assert_eq!(super::live_panes_in(&dir), vec!["a".to_string(), "b".to_string()]);
    assert!(!dir.join("dead.sock").exists(), "dead socket not swept");

    // One of them has a client attached, as the app would.
    let _app = attach(&dir.join("a.sock"), 80, 24).unwrap();
    assert!(super::end_all_in(&dir, PATIENCE).is_empty());
    for h in holders {
        h.join().unwrap().unwrap();
    }
    assert!(super::live_panes_in(&dir).is_empty());
}

#[test]
fn orphans_are_live_holders_the_session_does_not_name() {
    let live = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let session = vec!["b".to_string(), "z".to_string()];
    assert_eq!(super::orphans(&live, &session), vec!["a".to_string(), "c".to_string()]);
    assert!(super::orphans(&[], &session).is_empty());
}
