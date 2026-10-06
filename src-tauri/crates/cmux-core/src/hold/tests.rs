//! A real holder around a real shell, driven over its socket or pipe.
//! These run on every platform CI builds: on Windows they are the only
//! place the holder ever runs before a user turns `persistSessions` on.
//!
//! Shell commands come in two spellings, POSIX for unix and PowerShell for
//! Windows (what a command pane runs there).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::client::{attach, Attached};
use super::conn;
use super::server::{serve, HoldConfig, Program};
use super::wire::{read_frame, write_frame, Frame};

/// A cold PowerShell start on a CI runner is not fast.
const PATIENCE: Duration = Duration::from_secs(if cfg!(windows) { 60 } else { 20 });

/// The POSIX or the PowerShell spelling of a test's command.
fn sh(posix: &str, pwsh: &str) -> String {
    if cfg!(windows) { pwsh } else { posix }.to_string()
}

/// Creates a file — the marker a command leaves as its last act, so a test
/// knows it ran without guessing how long a shell takes to start.
fn touch(path: &Path) -> String {
    sh(
        &format!("touch '{}'", path.display()),
        &format!("New-Item -ItemType File -Force -Path '{}' | Out-Null", path.display()),
    )
}

fn marker(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "mh-{name}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_file(&path);
    path
}

fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + PATIENCE;
    while !path.exists() {
        assert!(Instant::now() < deadline, "{} never appeared", path.display());
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = std::fs::remove_file(path);
}

/// A private holder location per test. On unix a directory, short (socket
/// paths are capped at ~104 bytes) and 0700, which the holder insists on;
/// on Windows a pipe-name prefix.
fn location() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let id = format!("mh-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst));
    if cfg!(windows) {
        PathBuf::from(format!(r"\\.\pipe\{id}"))
    } else {
        let dir = PathBuf::from(format!("/tmp/{id}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }
}

fn socket() -> PathBuf {
    conn::endpoint(&location(), "p")
}

fn config(socket: &Path, command: &str, grace: Duration) -> HoldConfig {
    HoldConfig {
        pane: "test-pane".into(),
        socket: socket.to_path_buf(),
        cwd: None,
        cols: 80,
        rows: 24,
        program: Program::Command(command.into()),
        replay_bytes: HoldConfig::DEFAULT_REPLAY_BYTES,
        exited_grace: grace,
    }
}

fn spawn_at(socket: &Path, command: &str, grace: Duration) -> JoinHandle<io::Result<Option<i32>>> {
    let config = config(socket, command, grace);
    let holder = std::thread::spawn(move || serve(config));
    let deadline = Instant::now() + PATIENCE;
    while !conn::is_alive(socket) {
        assert!(Instant::now() < deadline, "holder never listened on {}", socket.display());
        std::thread::sleep(Duration::from_millis(10));
    }
    holder
}

fn start(command: &str, grace: Duration) -> (PathBuf, JoinHandle<io::Result<Option<i32>>>) {
    let path = socket();
    let holder = spawn_at(&path, command, grace);
    (path, holder)
}

fn connect(path: &Path) -> Attached {
    connect_sized(path, 80, 24)
}

fn connect_sized(path: &Path, cols: u16, rows: u16) -> Attached {
    let a = attach(path, cols, rows).expect("attach");
    a.stream.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
    a
}

/// What a terminal does with live output that asks for the cursor: answer.
/// ConPTY asks before it starts the shell, and an attached client is the
/// terminal — the holder only answers while nobody is.
fn answer_queries(a: &mut Attached, bytes: &[u8]) {
    if bytes.windows(4).any(|w| w == b"\x1b[6n") {
        let _ = write_frame(&mut a.stream, &Frame::Input(b"\x1b[1;1R".to_vec()));
    }
}

fn timed_out(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut)
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
            Ok(Some(Frame::Output(bytes))) => {
                answer_queries(a, &bytes);
                seen.extend(bytes);
            }
            Ok(Some(Frame::Exited(code))) => {
                return (String::from_utf8_lossy(&seen).into_owned(), Some(code))
            }
            Ok(None) => return (String::from_utf8_lossy(&seen).into_owned(), None),
            Ok(Some(_)) => {}
            Err(e) if timed_out(&e) => {}
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
            Ok(Some(Frame::Output(bytes))) => answer_queries(a, &bytes),
            Ok(None) => panic!("EOF before Exited"),
            _ => {}
        }
    }
}

fn send(a: &mut Attached, frame: Frame) {
    write_frame(&mut a.stream, &frame).unwrap();
}

fn kill_and_join(mut a: Attached, holder: JoinHandle<io::Result<Option<i32>>>) {
    send(&mut a, Frame::Kill);
    wait_exit(&mut a);
    holder.join().unwrap().unwrap();
}

#[test]
fn output_input_and_exit_reach_the_client() {
    let (path, holder) = start(
        &sh(
            "echo hello; read x; echo got:$x",
            r#"echo hello; $x = Read-Host; echo "got:$x""#,
        ),
        PATIENCE,
    );
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
    assert!(!conn::is_alive(&path), "still listening after its child exited");
}

/// The holder's last word must survive its closing the connection. On
/// Windows a disconnect discards whatever the client hasn't read, so this
/// client reads nothing until well after the holder is gone: the `Exited`
/// frame has to be waiting for it anyway.
#[test]
fn the_exit_frame_outlives_the_holder_closing() {
    let (path, holder) = start(&sh("echo ready; sleep 30", "echo ready; Start-Sleep 30"), PATIENCE);
    let mut a = connect(&path);
    let replay = std::mem::take(&mut a.replay);
    read_until(&mut a, replay, "ready");
    send(&mut a, Frame::Kill);
    // Not reading: let the holder write `Exited`, close, and finish first.
    holder.join().unwrap().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    wait_exit(&mut a);
}

#[test]
fn reattach_replays_what_was_printed_while_detached() {
    let printed = marker("two");
    let (path, holder) = start(
        &sh(
            &format!("echo one; sleep 1; echo two; {}; sleep 30", touch(&printed)),
            &format!("echo one; Start-Sleep 1; echo two; {}; Start-Sleep 30", touch(&printed)),
        ),
        PATIENCE,
    );
    let mut a = connect(&path);
    let replay = std::mem::take(&mut a.replay);
    read_until(&mut a, replay, "one");
    send(&mut a, Frame::Detach);
    drop(a);

    wait_for_file(&printed);
    let b = connect(&path);
    let replay = String::from_utf8_lossy(&b.replay).into_owned();
    assert!(replay.contains("one") && replay.contains("two"), "replay: {replay:?}");
    kill_and_join(b, holder);
}

#[test]
fn an_exit_while_detached_waits_for_a_client_to_collect_it() {
    let done = marker("done");
    let (path, holder) = start(&format!("echo bye; {}", touch(&done)), PATIENCE);
    wait_for_file(&done);
    std::thread::sleep(Duration::from_millis(1000));
    // Nobody attached when it exited, and the holder is still waiting.
    assert!(!holder.is_finished(), "holder quit before anyone saw the exit");

    let mut a = connect(&path);
    assert_eq!(a.exit_code, Some(0));
    assert!(String::from_utf8_lossy(&a.replay).contains("bye"));
    assert_eq!(wait_exit(&mut a), Some(0));
    assert_eq!(holder.join().unwrap().unwrap(), Some(0));
}

#[test]
fn an_uncollected_exit_gives_up_after_the_grace_period() {
    let (_path, holder) = start(&sh("true", "exit 0"), Duration::from_millis(300));
    let deadline = Instant::now() + PATIENCE;
    while !holder.is_finished() {
        assert!(Instant::now() < deadline, "holder outlived its grace period");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(holder.join().unwrap().unwrap(), Some(0));
}

#[test]
fn a_new_client_displaces_the_old_one() {
    let (path, holder) = start(&sh("echo ready; sleep 30", "echo ready; Start-Sleep 30"), PATIENCE);
    let mut first = connect(&path);
    let replay = std::mem::take(&mut first.replay);
    read_until(&mut first, replay, "ready");

    let second = connect(&path);
    let deadline = Instant::now() + PATIENCE;
    loop {
        assert!(Instant::now() < deadline, "first client never cut off");
        // The holder shuts the old connection down: EOF, or an error.
        match read_frame(&mut first.stream) {
            Ok(None) => break,
            Err(e) if timed_out(&e) => {}
            Err(_) => break,
            Ok(Some(_)) => {}
        }
    }
    kill_and_join(second, holder);
}

#[test]
fn the_attach_size_reaches_the_child() {
    let (path, holder) = start(
        &sh(
            "sleep 1; stty size; sleep 30",
            r#"Start-Sleep 1; $s = $Host.UI.RawUI.WindowSize; echo "$($s.Height) $($s.Width)"; Start-Sleep 30"#,
        ),
        PATIENCE,
    );
    let mut a = connect_sized(&path, 100, 30);
    let replay = std::mem::take(&mut a.replay);
    read_until(&mut a, replay, "30 100");
    kill_and_join(a, holder);
}

#[test]
fn a_live_holder_is_never_taken_over() {
    let (path, holder) = start(&sh("sleep 30", "Start-Sleep 30"), PATIENCE);
    let err = serve(config(&path, &sh("true", "exit 0"), PATIENCE)).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::AddrInUse);

    let a = connect(&path);
    kill_and_join(a, holder);
}

/// Leaves a socket file nobody listens on. Not just bind-then-drop: on
/// macOS a socket is made close-on-exec a moment after it is created, so a
/// child that another test forks in that moment inherits it and keeps it
/// answering until the child exits. Wait that out.
#[cfg(unix)]
fn stale_socket(path: &Path) {
    use std::os::unix::net::{UnixListener, UnixStream};
    drop(UnixListener::bind(path).unwrap());
    let deadline = Instant::now() + PATIENCE;
    while UnixStream::connect(path).is_ok() {
        assert!(Instant::now() < deadline, "{} never went stale", path.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
fn private_dir(dir: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// A socket file outlives a holder that died; a pipe does not, so there is
/// nothing like this on Windows.
#[cfg(unix)]
#[test]
fn a_stale_socket_file_is_replaced() {
    let path = socket();
    private_dir(path.parent().unwrap(), 0o700);
    stale_socket(&path);
    assert!(path.exists());
    assert_eq!(serve(config(&path, "echo fresh", Duration::from_millis(200))).unwrap(), Some(0));
}

/// The directory is unix's security boundary; Windows pipes get theirs from
/// the default security descriptor.
#[cfg(unix)]
#[test]
fn a_directory_others_can_enter_is_refused() {
    let path = socket();
    private_dir(path.parent().unwrap(), 0o755);
    let err = serve(config(&path, "true", Duration::from_millis(200))).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn notifications_raised_while_detached_are_reported_once() {
    // A marker, not polling: every attach counts as being seen live, so
    // checking by attaching could swallow the very notification under test.
    let sent = marker("sent");
    let (path, holder) = start(
        &sh(
            &format!(
                "echo up; read a; printf '\\033]777;notify;Claude Code;finished responding\\007'; \
                 {}; read b; printf '\\033]9;seen live\\007'; echo live; read c",
                touch(&sent)
            ),
            &format!(
                "echo up; $a = Read-Host; \
                 Write-Host -NoNewline \"$([char]27)]777;notify;Claude Code;finished responding$([char]7)\"; \
                 {}; $b = Read-Host; \
                 Write-Host -NoNewline \"$([char]27)]9;seen live$([char]7)\"; echo live; $c = Read-Host",
                touch(&sent)
            ),
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
    wait_for_file(&sent);

    let mut b = connect(&path);
    assert_eq!(b.missed, 1);
    assert_eq!(b.last_missed.as_deref(), Some("Claude Code: finished responding"));

    // One raised while attached was seen live: not counted for next time.
    send(&mut b, Frame::Input(b"go\r".to_vec()));
    read_until(&mut b, Vec::new(), "live");
    send(&mut b, Frame::Detach);
    drop(b);
    let c = connect(&path);
    assert_eq!((c.missed, c.last_missed.as_deref()), (0, None));
    kill_and_join(c, holder);
}

#[test]
fn end_all_ends_every_holder_it_finds() {
    let loc = location();
    let holders: Vec<_> = ["a", "b"]
        .iter()
        .map(|pane| spawn_at(&conn::endpoint(&loc, pane), &sh("sleep 60", "Start-Sleep 60"), PATIENCE))
        .collect();
    assert_eq!(conn::live_panes(&loc), vec!["a".to_string(), "b".to_string()]);

    // One of them has a client attached, as the app would.
    let _app = attach(&conn::endpoint(&loc, "a"), 80, 24).unwrap();
    assert!(super::end_all_in(&loc, PATIENCE).is_empty());
    for h in holders {
        h.join().unwrap().unwrap();
    }
    assert!(conn::live_panes(&loc).is_empty());
}

#[cfg(unix)]
#[test]
fn listing_sweeps_dead_socket_files() {
    let loc = location();
    let live = spawn_at(&conn::endpoint(&loc, "a"), "sleep 60", PATIENCE);
    stale_socket(&loc.join("dead.sock"));
    assert_eq!(conn::live_panes(&loc), vec!["a".to_string()]);
    assert!(!loc.join("dead.sock").exists(), "dead socket not swept");
    kill_and_join(attach(&conn::endpoint(&loc, "a"), 80, 24).unwrap(), live);
}

/// A program that asks where the cursor is, with nobody attached, would
/// otherwise wait forever: the holder answers in the terminal's place. On
/// Windows every test above that starts a shell before attaching depends
/// on this, because ConPTY asks before it runs the shell at all.
#[cfg(unix)]
#[test]
fn a_cursor_query_while_detached_is_answered() {
    let replied = marker("replied");
    // Raw mode, ask, and block until six bytes of reply arrive.
    let (path, holder) = start(
        &format!(
            "stty raw -echo; printf '\\033[6n'; dd bs=6 count=1 >/dev/null 2>&1; \
             stty sane; {}; sleep 30",
            touch(&replied)
        ),
        PATIENCE,
    );
    wait_for_file(&replied);
    kill_and_join(connect(&path), holder);
}

#[test]
fn orphans_are_live_holders_the_session_does_not_name() {
    let live = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let session = vec!["b".to_string(), "z".to_string()];
    assert_eq!(super::orphans(&live, &session), vec!["a".to_string(), "c".to_string()]);
    assert!(super::orphans(&[], &session).is_empty());
}
