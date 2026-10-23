//! The task log, end to end: what Mirador records as agents are given work
//! and end their turns, read back through the real `mira task list` and
//! `mira agent history`. Skipped unless `MIRA_E2E=1`.

use std::time::Duration;

use cmux_e2e::{e2e, Sandbox};
use cmux_protocol::Request;

const LONG: Duration = Duration::from_secs(45);

fn pane_of(sb: &Sandbox, name: &str) -> String {
    sb.member(name).and_then(|m| m["pane"].as_str().map(String::from)).expect(name)
}

/// `mira <args>` from `pane`, which must succeed; its output.
fn mira(sb: &Sandbox, pane: &str, args: &[&str]) -> String {
    let (code, out, err) = sb.mira(Some(pane), args);
    if code != 0 {
        sb.fail(&format!("mira {args:?}: {err}"));
    }
    out
}

/// The column of the board `mira task list` shows a card titled `title` in.
fn column_of(board: &str, title: &str) -> Option<String> {
    let mut column = None;
    for line in board.lines() {
        if !line.starts_with(' ') {
            column = Some(line.to_string());
        } else if line.ends_with(title) {
            return column;
        }
    }
    None
}

#[test]
fn tasks_are_logged_as_agents_work() {
    e2e!();
    let mut sb = Sandbox::new("tasks");
    sb.set_env("E2E_TURN_MS", "3000");
    sb.write(".mirador/roles/planner.md", "---\nmodel: opus\n---\nYou plan.\n");
    sb.write(".mirador/roles/reviewer.md", "---\nmodel: haiku\n---\nYou review.\n");
    sb.write(
        ".mirador/teams/squad.md",
        "---\nmembers:\n  - role: planner\n    lead: true\n  - role: reviewer\n---\nShip small.\n",
    );
    sb.launch();
    let shell = sb.first_pane();
    sb.call(Request::TeamStart { team: "squad".into(), task: Some("plan the chart".into()), pane_id: Some(shell) });
    sb.wait_for("the reviewer", LONG, || sb.member("reviewer"));
    let planner = pane_of(&sb, "planner");
    let reviewer = pane_of(&sb, "reviewer");
    let session = sb.wait_for("the reviewer's session", LONG, || {
        sb.events("hooked", &reviewer).into_iter().next()?["session"].as_str().map(String::from)
    });
    sb.wait_for("the planner's session", LONG, || sb.events("hooked", &planner).into_iter().next());

    // The team's task is the lead's card, given by a person.
    let board = mira(&sb, &planner, &["task", "list"]);
    assert_eq!(column_of(&board, "plan the chart").as_deref(), Some("Doing"), "{board}");

    // A message between members is a card that follows the turn it starts.
    mira(&sb, &planner, &["agent", "send", "reviewer", "Review step 1\nthe model"]);
    let board = mira(&sb, &planner, &["task", "list"]);
    assert_eq!(column_of(&board, "Review step 1").as_deref(), Some("Doing"), "{board}");
    sb.wait_for("the review to be done", LONG, || {
        let board = mira(&sb, &planner, &["task", "list"]);
        (column_of(&board, "Review step 1").as_deref() == Some("Done")).then_some(())
    });
    let history = mira(&sb, &planner, &["agent", "history", "reviewer"]);
    assert!(history.contains("planner → reviewer  agent-send  Review step 1"), "{history}");
    assert!(history.contains("reviewer done after "), "{history}");
    assert!(!history.contains("plan the chart"), "the planner's task is not the reviewer's: {history}");

    // Asking a person, then going back to work once answered.
    mira(&sb, &planner, &["agent", "send", "reviewer", "!ask may I run the tests"]);
    sb.wait_for("the reviewer to need you", LONG, || {
        let board = mira(&sb, &planner, &["task", "list"]);
        (column_of(&board, "!ask may I run the tests").as_deref() == Some("Needs you")).then_some(())
    });
    // What Claude Code reports once the permission is granted and the tool ran.
    sb.call(Request::AgentSession {
        pane_id: Some(reviewer.clone()),
        tty: None,
        agent: "claude".into(),
        session_id: session,
        job: None,
        event: Some("post-tool".into()),
        transcript_path: None,
        notification_type: None,
        message: None,
    });
    let board = mira(&sb, &planner, &["task", "list"]);
    assert_eq!(column_of(&board, "!ask may I run the tests").as_deref(), Some("Doing"), "{board}");
    let history = mira(&sb, &planner, &["agent", "history"]);
    assert!(history.contains("reviewer needs-you after") && history.contains("reviewer resumed"), "{history}");

    // The log outlives the app.
    sb.quit();
    sb.launch();
    let board = mira(&sb, &planner, &["task", "list"]);
    assert_eq!(column_of(&board, "Review step 1").as_deref(), Some("Done"), "{board}");
    assert!(mira(&sb, &planner, &["agent", "history", "--all"]).contains("you → planner  team-start  plan the chart"));
}
