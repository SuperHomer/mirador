//! `mira claude-statusline`, as Claude Code runs it in front of a user's
//! own status line: the user's command gets the same stdin, its output is
//! ours, and the plan's limits are kept on the side.

#![cfg(unix)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const STATUSLINE: &str =
    r#"{"model":{"display_name":"Opus"},"rate_limits":{"five_hour":{"used_percentage":40,"resets_at":1795590902}}}"#;

/// Runs the real `mira claude-statusline` with `args`, `input` on its
/// stdin and `home` as its home: its stdout and exit code.
fn statusline(home: &Path, args: &[&str], input: &str) -> (String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mira"))
        .arg("claude-statusline")
        .args(args)
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.stderr.is_empty(), "nothing on stderr: {}", String::from_utf8_lossy(&out.stderr));
    (String::from_utf8(out.stdout).unwrap(), out.status.code().unwrap())
}

/// Wherever the platform's data directory puts it, under `home`.
fn snapshot(home: &Path) -> Option<PathBuf> {
    fn find(dir: &Path) -> Option<PathBuf> {
        std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
            let path = e.path();
            if path.is_dir() {
                find(&path)
            } else {
                (path.file_name()? == "claude-limits.json").then_some(path)
            }
        })
    }
    find(home)
}

fn temp_home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mira-statusline-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_users_status_line_gets_the_same_input_and_its_output_is_ours() {
    let home = temp_home("run");
    let (out, code) = statusline(&home, &["--run", "printf 'line: '; cat; exit 3"], STATUSLINE);
    assert_eq!(out, format!("line: {STATUSLINE}"));
    assert_eq!(code, 3, "the command's exit code is ours");
    let saved = cmux_core::usage::read_claude_limits(&snapshot(&home).expect("a snapshot")).unwrap();
    assert_eq!((saved.windows.len(), saved.windows[0].used_percent), (1, 40.0));
}

#[test]
fn without_a_command_it_prints_nothing_and_input_it_cannot_read_is_no_error() {
    let home = temp_home("bare");
    assert_eq!(statusline(&home, &[], STATUSLINE), (String::new(), 0));
    assert!(snapshot(&home).is_some());
    let other = temp_home("garbage");
    assert_eq!(statusline(&other, &["--run", "echo still"], "{not json"), ("still\n".to_string(), 0));
    assert_eq!(snapshot(&other), None, "nothing to keep");
}
