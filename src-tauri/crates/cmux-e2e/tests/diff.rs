//! The diff pane limited to one file, end to end: `mira diff --path`.
//! Skipped unless `MIRA_E2E=1` (see the crate docs for what to build
//! first).

use cmux_e2e::{e2e, Sandbox};
use cmux_protocol::Request;

/// Sorted: the snapshot lists them in no particular order.
fn diff_panes(sb: &Sandbox) -> Vec<(String, Option<String>)> {
    let mut panes: Vec<_> = sb.snapshot()["diffPanes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| (d["spec"].as_str().unwrap().to_string(), d["path"].as_str().map(String::from)))
        .collect();
    panes.sort();
    panes
}

/// A path relative to where `mira` ran, or absolute, reaches the pane as
/// one relative to the repository; a deleted file is still one to diff,
/// and a path outside the repository is refused by the app, whoever sends
/// it.
#[test]
fn mira_diff_path_shows_one_file() {
    e2e!();
    let mut sb = Sandbox::new("diffpath");
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git")
            .args(["-c", "user.name=e2e", "-c", "user.email=e2e@example.com", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(&sb.project)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q"]);
    sb.write("src/a.txt", "one\n");
    sb.write("gone.txt", "one\n");
    git(&["add", "."]);
    git(&["commit", "-qm", "init"]);
    sb.write("src/a.txt", "one\ntwo\n");
    std::fs::remove_file(sb.project.join("gone.txt")).unwrap();
    sb.launch();
    let first = sb.first_pane();

    let out = sb
        .mira_command(Some(&first), &["diff", "--path", "a.txt"])
        .current_dir(sb.project.join("src"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let gone = sb.project.join("gone.txt").to_string_lossy().into_owned();
    let (code, _, err) = sb.mira(Some(&first), &["diff", "--path", &gone]);
    assert_eq!(code, 0, "a deleted file: {err}");
    assert_eq!(
        diff_panes(&sb),
        [("worktree".to_string(), Some("gone.txt".to_string())), ("worktree".to_string(), Some("src/a.txt".to_string()))],
    );

    let outside = std::env::temp_dir().join("elsewhere.txt").to_string_lossy().into_owned();
    let (code, _, err) = sb.mira(Some(&first), &["diff", "--path", &outside]);
    assert_ne!(code, 0);
    assert!(err.contains("is not in"), "{err}");
    let refused = sb.request(Request::DiffOpen {
        spec: None,
        target: None,
        pane_id: Some(first.clone()),
        worktree: None,
        path: Some("../elsewhere.txt".into()),
    });
    assert!(refused.unwrap_err().contains("not a path inside"));
    assert_eq!(diff_panes(&sb).len(), 2);
}
