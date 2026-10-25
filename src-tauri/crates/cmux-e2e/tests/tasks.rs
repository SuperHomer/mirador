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

/// The id `mira task list` shows for the card titled `title`.
fn id_of(board: &str, title: &str) -> String {
    let line = board.lines().find(|l| l.starts_with(' ') && l.ends_with(title)).expect(title);
    line.split_whitespace().next().unwrap().to_string()
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

    // A card written by hand, for a teammate, moved by hand.
    let id = mira(&sb, &planner, &["task", "add", "--to", "reviewer", "Step 2: write the docs"]).trim().to_string();
    let board = mira(&sb, &planner, &["task", "list"]);
    assert_eq!(column_of(&board, "Step 2: write the docs").as_deref(), Some("To do"), "{board}");
    assert_eq!(id_of(&board, "Step 2: write the docs"), id, "{board}");
    assert!(board.contains(&format!("{id}  reviewer ")), "{board}");
    mira(&sb, &planner, &["task", "move", &id, "doing"]);
    let board = mira(&sb, &planner, &["task", "list"]);
    assert_eq!(column_of(&board, "Step 2: write the docs").as_deref(), Some("Doing"), "{board}");
    mira(&sb, &planner, &["task", "done", &id[..5]]);
    let board = mira(&sb, &planner, &["task", "list"]);
    assert_eq!(column_of(&board, "Step 2: write the docs").as_deref(), Some("Done"), "{board}");
    let history = mira(&sb, &planner, &["agent", "history"]);
    assert!(history.contains(&format!("planner moved {id} to doing")), "{history}");

    // A name that is no teammate is no card; a card that follows its agent
    // is not moved by hand.
    let (code, _, err) = sb.mira(Some(&planner), &["task", "add", "--to", "reveiwer", "typo"]);
    assert!(code != 0 && err.contains("no agent `reveiwer`"), "{err}");
    assert!(!mira(&sb, &planner, &["task", "list"]).contains("typo"));
    let auto = id_of(&mira(&sb, &planner, &["task", "list"]), "Review step 1");
    let (code, _, err) = sb.mira(Some(&planner), &["task", "move", &auto, "todo"]);
    assert!(code != 0 && err.contains("follows reviewer"), "{err}");

    // A message sent for a written card is that card's: in its history,
    // with no card of its own, and the card stays where it was put.
    let step3 = mira(&sb, &planner, &["task", "add", "--to", "reviewer", "Step 3: the board"]).trim().to_string();
    let text = format!("Step 3 (card {step3}): build it");
    mira(&sb, &planner, &["agent", "send", "reviewer", "--card", &step3[..6], &text]);
    sb.wait_for("the reviewer's turn on step 3", LONG, || {
        let history = mira(&sb, &planner, &["agent", "history", "reviewer"]);
        history.lines().last().filter(|l| l.contains("reviewer done after")).map(|_| ())
    });
    let board = mira(&sb, &planner, &["task", "list"]);
    assert_eq!(column_of(&board, "Step 3: the board").as_deref(), Some("To do"), "{board}");
    assert_eq!(column_of(&board, &text), None, "no card of its own: {board}");
    let cards = sb.call(Request::TaskList { all: true, pane_id: None });
    let card = cards["cards"].as_array().unwrap().iter().find(|c| c["id"] == step3.as_str()).unwrap().clone();
    let texts: Vec<&str> = card["history"].as_array().unwrap().iter().filter_map(|e| e["text"].as_str()).collect();
    assert_eq!(texts, ["Step 3: the board", text.as_str()], "{card}");
    // The card's history, as its board opens it: the send is on it.
    let lines = mira(&sb, &planner, &["agent", "history", "--card", &step3]);
    let lines: Vec<&str> = lines.lines().map(|l| l.split_once(" ago  ").map_or(l, |(_, what)| what)).collect();
    assert_eq!(
        lines,
        ["planner → reviewer  manual  Step 3: the board", &format!("planner → reviewer  agent-send  {text}")],
        "{lines:?}"
    );
    // The tab's history, as its board's History view reads it: the sends
    // between the two members, and how the turns ended.
    let tab_lines = mira(&sb, &planner, &["agent", "history"]);
    assert!(tab_lines.contains("planner → reviewer  agent-send  Review step 1"), "{tab_lines}");
    assert!(tab_lines.contains("reviewer done after"), "{tab_lines}");

    // A card that is not one written by hand refuses the send: nothing typed.
    let before = sb.events("message", &reviewer).len();
    let (code, _, err) = sb.mira(Some(&planner), &["agent", "send", "reviewer", "--card", "zzzz", "hello"]);
    assert!(code != 0 && err.contains("no card `zzzz`"), "{err}");
    let (code, _, err) = sb.mira(Some(&planner), &["agent", "send", "reviewer", "--card", &auto, "hello"]);
    assert!(code != 0 && err.contains("follows reviewer"), "{err}");
    // A message sent after them is the next the reviewer gets.
    mira(&sb, &planner, &["agent", "send", "reviewer", "after the refusals"]);
    let got = sb.wait_for("the message after the refusals", LONG, || {
        let got = sb.events("message", &reviewer);
        (got.len() > before).then_some(got)
    });
    let texts: Vec<&str> = got[before..].iter().filter_map(|m| m["text"].as_str()).collect();
    assert_eq!(texts.len(), 1, "the refused sends typed nothing: {texts:?}");
    assert!(texts[0].ends_with("after the refusals"), "{texts:?}");

    // The log outlives the app.
    sb.quit();
    sb.launch();
    let board = mira(&sb, &planner, &["task", "list"]);
    assert_eq!(column_of(&board, "Review step 1").as_deref(), Some("Done"), "{board}");
    assert!(mira(&sb, &planner, &["agent", "history", "--all"]).contains("you → planner  team-start  plan the chart"));
}

/// The board pane opens beside the pane it is asked from, showing that
/// tab's cards, and is still a board after a relaunch. What it draws is
/// the webview's, out of reach here; the cards it reads are the ones
/// `task list` prints, from the same function.
#[test]
fn the_task_board_opens_and_comes_back() {
    e2e!();
    let mut sb = Sandbox::new("task-board");
    sb.launch();
    let first = sb.first_pane();
    let tab = sb.snapshot()["activeTab"].as_str().unwrap().to_string();
    mira(&sb, &first, &["task", "add", "Step 1: the pane"]);

    let board = mira(&sb, &first, &["agent", "board"]).trim().to_string();
    let listed = |sb: &Sandbox| -> Option<serde_json::Value> {
        sb.snapshot()["taskBoardPanes"].as_array()?.iter().find(|b| b["paneId"] == board.as_str()).cloned()
    };
    let pane = sb.wait_for("the board pane", LONG, || listed(&sb));
    assert_eq!(pane["tabId"], tab.as_str(), "{pane}");
    assert_eq!(sb.panes(), [first.clone(), board.clone()]);
    // A plain agent, with no team, session or role name, is called by its
    // harness, as `agent list` calls it.
    mira(&sb, &first, &["agent", "new", "Upgrade the logging library"]);
    let list = sb.wait_for("the plain agent's card", LONG, || {
        let list = mira(&sb, &first, &["task", "list"]);
        list.contains("Upgrade the logging library").then_some(list)
    });
    let line = list.lines().find(|l| l.ends_with("Upgrade the logging library")).unwrap();
    assert_eq!(line.split_whitespace().nth(1), Some("claude"), "{list}");
    // A render throw in the board takes the whole tree down with it: the
    // terminal beside it still answering says the page survived it.
    sb.type_into(&first, "echo still-$((40+2))\r");
    sb.wait_for("the terminal beside the board", LONG, || {
        sb.screen(&first).filter(|s| s.lines().any(|l| l.trim() == "still-42"))
    });
    // Asked from the board itself, its tab is still the one it shows.
    let cards = sb.call(Request::TaskList { all: false, pane_id: Some(board.clone()) });
    let titles: Vec<&str> = cards["cards"].as_array().unwrap().iter().filter_map(|c| c["title"].as_str()).collect();
    assert_eq!(titles, ["Step 1: the pane", "Upgrade the logging library"]);

    // One of its own, in a tab of its own, still showing this tab.
    let other = mira(&sb, &first, &["agent", "board", "--tab"]).trim().to_string();
    let other_pane = sb.wait_for("the board in a tab", LONG, || {
        sb.snapshot()["taskBoardPanes"].as_array()?.iter().find(|b| b["paneId"] == other.as_str()).cloned()
    });
    assert_eq!(other_pane["tabId"], tab.as_str());
    assert_eq!(sb.snapshot()["tabs"].as_array().unwrap().len(), 2);

    sb.quit();
    sb.launch();
    let pane = listed(&sb).expect("the board comes back as a board, not a shell");
    assert_eq!(pane["tabId"], tab.as_str());
}

#[test]
fn a_codex_agent_s_card_follows_its_turns() {
    e2e!();
    let mut sb = Sandbox::new("codextasks");
    sb.set_env("E2E_TURN_MS", "3000");
    sb.write(".mirador/roles/dev.md", "---\nmodel: opus\n---\nYou build.\n");
    sb.write(".mirador/roles/coder.md", "---\nharness: codex\nmodel: gpt-5\n---\nYou code.\n");
    sb.write(
        ".mirador/teams/pair.md",
        "---\nmembers:\n  - role: dev\n    lead: true\n  - role: coder\n---\nShip small.\n",
    );
    sb.launch();
    let shell = sb.first_pane();
    sb.call(Request::TeamStart { team: "pair".into(), task: None, pane_id: Some(shell) });
    sb.wait_for("the coder", LONG, || sb.member("coder"));
    let dev = pane_of(&sb, "dev");
    let coder = pane_of(&sb, "coder");
    sb.wait_for("the dev's session", LONG, || sb.events("hooked", &dev).into_iter().next());

    // Codex has no hooks: its turn ends come from its rollout.
    mira(&sb, &dev, &["agent", "send", "coder", "Refactor the parser"]);
    let board = mira(&sb, &dev, &["task", "list"]);
    assert_eq!(column_of(&board, "Refactor the parser").as_deref(), Some("Doing"), "{board}");
    let board = sb.wait_for("the card to be done", LONG, || {
        let board = mira(&sb, &dev, &["task", "list"]);
        (column_of(&board, "Refactor the parser").as_deref() == Some("Done")).then_some(board)
    });
    let history = mira(&sb, &dev, &["agent", "history", "--pane", &coder]);
    assert_eq!(history.lines().filter(|l| l.contains("coder done")).count(), 1, "one end for its one turn:\n{history}\n{board}");
}

#[test]
fn a_codex_turn_right_after_new_is_logged_once() {
    e2e!();
    let mut sb = Sandbox::new("codexnew");
    // Turns shorter than the poller's tick.
    sb.set_env("E2E_TURN_MS", "100");
    sb.write(".mirador/roles/dev.md", "---\nmodel: opus\n---\nYou build.\n");
    sb.write(".mirador/roles/coder.md", "---\nharness: codex\nmodel: gpt-5\n---\nYou code.\n");
    sb.write(
        ".mirador/teams/pair.md",
        "---\nmembers:\n  - role: dev\n    lead: true\n  - role: coder\n---\nShip small.\n",
    );
    sb.launch();
    let shell = sb.first_pane();
    sb.call(Request::TeamStart { team: "pair".into(), task: None, pane_id: Some(shell.clone()) });
    sb.wait_for("the coder", LONG, || sb.member("coder"));
    let dev = pane_of(&sb, "dev");
    let coder = pane_of(&sb, "coder");
    sb.wait_for("the dev's session", LONG, || sb.events("hooked", &dev).into_iter().next());
    let done = |title: &str| {
        sb.wait_for(&format!("{title} to be done"), LONG, || {
            let board = mira(&sb, &dev, &["task", "list"]);
            (column_of(&board, title).as_deref() == Some("Done")).then_some(())
        })
    };

    // Its first session, followed.
    mira(&sb, &dev, &["agent", "send", "coder", "First pass"]);
    done("First pass");

    // A new session, and a turn in it over before the poller next looks:
    // the new rollout already holds its end when it is first read. `/new`
    // typed by a person: from an agent it would come headed with who sent it.
    mira(&sb, &shell, &["agent", "send", &coder, "/new"]);
    mira(&sb, &dev, &["agent", "send", "coder", "Quick fix"]);
    done("Quick fix");
    let history = mira(&sb, &dev, &["agent", "history", "--pane", &coder]);
    assert_eq!(history.lines().filter(|l| l.contains("coder done")).count(), 2, "one end per turn:\n{history}");
}
