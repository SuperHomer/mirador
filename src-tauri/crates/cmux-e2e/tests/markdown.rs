//! The markdown pane, end to end: `mira md` opens one, and it follows its
//! file. Skipped unless `MIRA_E2E=1` (see the crate docs for what to build
//! first).
//!
//! What the pane shows is read with `read-screen`, which a markdown pane
//! answers with the text it rendered: from the page, after its re-read, so
//! a change seen there reached the page, not only Rust.

use std::time::Duration;

use cmux_e2e::{e2e, Sandbox};
use cmux_protocol::Request;

const LONG: Duration = Duration::from_secs(45);

fn markdown_panes(sb: &Sandbox) -> Vec<(String, String)> {
    sb.snapshot()["markdownPanes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| (m["paneId"].as_str().unwrap().to_string(), m["path"].as_str().unwrap().to_string()))
        .collect()
}

/// `mira md` opens a pane on the file; in a tab, a second ask for the same
/// file, by an absolute path or one relative to where `mira` ran, shows the
/// first one again. A file that is not there is refused, by the CLI and by
/// the app alike.
#[test]
fn mira_md_opens_a_file_once_and_refuses_a_missing_one() {
    e2e!();
    let mut sb = Sandbox::new("mdopen");
    sb.write("PLAN.md", "# Plan\n\nfirst draft\n");
    let plan = sb.project.join("PLAN.md").to_string_lossy().into_owned();
    sb.launch();
    let first = sb.first_pane();

    let (code, out, err) = sb.mira(Some(&first), &["md", &plan, "--tab"]);
    assert_eq!(code, 0, "{err}");
    let pane = out.trim().to_string();
    assert_eq!(markdown_panes(&sb), [(pane.clone(), plan.clone())]);
    sb.wait_for_screen(&pane, "the file's text", LONG, |s| s.contains("first draft"));

    let (code, out, err) = sb.mira(Some(&first), &["md", &plan, "--tab"]);
    assert_eq!((code, out.trim()), (0, pane.as_str()), "the same file again: {err}");
    let out = sb
        .mira_command(Some(&first), &["md", "PLAN.md", "--tab"])
        .current_dir(&sb.project)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), pane, "a relative path is the same file");
    assert_eq!(markdown_panes(&sb).len(), 1);

    let (code, _, err) = sb.mira(Some(&first), &["md", "nope.md"]);
    assert_ne!(code, 0);
    assert!(err.contains("no file nope.md"), "{err}");
    // The app checks for itself, whatever a client sends it.
    let missing = sb.project.join("nope.md").to_string_lossy().into_owned();
    let refused = sb.request(Request::MarkdownOpen { path: missing, target: None, pane_id: None });
    assert!(refused.unwrap_err().contains("no such file"));
    let refused = sb.request(Request::MarkdownOpen { path: "PLAN.md".into(), target: None, pane_id: None });
    assert!(refused.unwrap_err().contains("not an absolute path"));
    assert_eq!(markdown_panes(&sb).len(), 1);
}

/// The pane follows its file through the ways it changes on disk: written
/// in place, saved atomically (a new file renamed over it, as most editors
/// do), and deleted then written again, the error showing in between.
#[test]
fn a_markdown_pane_follows_its_file() {
    e2e!();
    let mut sb = Sandbox::new("mdfollow");
    sb.write("notes.md", "first draft\n");
    let notes = sb.project.join("notes.md");
    sb.launch();
    let first = sb.first_pane();
    let (code, out, err) = sb.mira(Some(&first), &["md", &notes.to_string_lossy()]);
    assert_eq!(code, 0, "{err}");
    let pane = out.trim().to_string();
    sb.wait_for_screen(&pane, "the file's text", LONG, |s| s.contains("first draft"));

    std::fs::write(&notes, "second draft\n").unwrap();
    sb.wait_for_screen(&pane, "a write in place", LONG, |s| s.contains("second draft"));

    let tmp = sb.project.join(".notes.md.swp");
    std::fs::write(&tmp, "third draft\n").unwrap();
    std::fs::rename(&tmp, &notes).unwrap();
    sb.wait_for_screen(&pane, "an atomic save", LONG, |s| s.contains("third draft"));

    std::fs::remove_file(&notes).unwrap();
    sb.wait_for_screen(&pane, "the read error", LONG, |s| s.contains("notes.md") && !s.contains("third draft"));
    std::fs::write(&notes, "fourth draft\n").unwrap();
    sb.wait_for_screen(&pane, "the file written again", LONG, |s| s.contains("fourth draft"));
}
