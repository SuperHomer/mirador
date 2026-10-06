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
