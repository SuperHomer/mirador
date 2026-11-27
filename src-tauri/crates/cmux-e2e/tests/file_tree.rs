//! The file tree pane, end to end: `mira tree` opens one, and `read-screen`
//! reads its rows. Skipped unless `MIRA_E2E=1` (see the crate docs for what
//! to build first).
//!
//! `read-screen` answers a tree pane with the text its body drew, read from
//! the page: a row seen there was listed in Rust and drawn, its status
//! letter included.

use std::time::Duration;

use cmux_e2e::{e2e, Sandbox};

const LONG: Duration = Duration::from_secs(45);

fn tree_panes(sb: &Sandbox) -> Vec<(String, String)> {
    sb.snapshot()["fileTreePanes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| (t["paneId"].as_str().unwrap().to_string(), t["root"].as_str().unwrap().to_string()))
        .collect()
}

/// The rows as one line: each name or letter is its own block, as the
/// row's flex items are.
fn words(screen: &str) -> String {
    screen.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `mira tree <repo> --tab` opens a pane listing what git sees, each file
/// with its status and the ignored one left out; asked again, by the same
/// directory or by one inside the repository, it shows that pane again.
/// A directory that is not there is refused by the CLI.
#[test]
fn mira_tree_lists_a_repository_once_with_its_status() {
    e2e!();
    let mut sb = Sandbox::new("treeopen");
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
    sb.write(".gitignore", "ignored.log\n");
    sb.write("clean.txt", "one\n");
    sb.write("edited.txt", "one\n");
    sb.write("sub/keep.txt", "one\n");
    git(&["add", "."]);
    git(&["commit", "-qm", "init"]);
    sb.write("edited.txt", "two\n");
    sb.write("new.txt", "fresh\n");
    sb.write("ignored.log", "noise\n");
    let repo = sb.project.to_string_lossy().into_owned();
    sb.launch();
    let first = sb.first_pane();

    let (code, out, err) = sb.mira(Some(&first), &["tree", &repo, "--tab"]);
    assert_eq!(code, 0, "{err}");
    let pane = out.trim().to_string();
    assert_eq!(tree_panes(&sb), [(pane.clone(), repo.clone())]);
    let screen = sb.wait_for_screen(&pane, "the tree's rows", LONG, |s| s.contains("new.txt"));
    let rows = words(&screen);
    // Directories first, then files; a status letter after its file.
    assert!(
        rows.contains("sub .gitignore clean.txt edited.txt M new.txt U"),
        "rows: {rows}"
    );
    assert!(!rows.contains("ignored.log"), "rows: {rows}");

    let (code, out, err) = sb.mira(Some(&first), &["tree", &repo, "--tab"]);
    assert_eq!((code, out.trim()), (0, pane.as_str()), "the same root again: {err}");
    // From inside the repository, the tree is the repository's.
    let out = sb
        .mira_command(Some(&first), &["tree", ".", "--tab"])
        .current_dir(sb.project.join("sub"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), pane, "a subdirectory roots at the repository");
    assert_eq!(tree_panes(&sb).len(), 1);

    let (code, _, err) = sb.mira(Some(&first), &["tree", "nope"]);
    assert_ne!(code, 0);
    assert!(err.contains("no directory nope"), "{err}");
    assert_eq!(tree_panes(&sb).len(), 1);
}
