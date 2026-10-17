//! Teams, end to end: started in a tab of their own, kept track of, and
//! brought back (#119). Skipped unless `MIRA_E2E=1`.

use std::time::Duration;

use cmux_e2e::{e2e, Sandbox};
use cmux_protocol::Request;

const LONG: Duration = Duration::from_secs(45);

fn state(sb: &Sandbox, name: &str) -> Option<String> {
    sb.member(name).and_then(|m| m["state"].as_str().map(String::from))
}

fn pane_of(sb: &Sandbox, name: &str) -> String {
    sb.member(name).and_then(|m| m["pane"].as_str().map(String::from)).expect(name)
}

#[test]
fn a_team_starts_is_tracked_and_comes_back() {
    e2e!();
    let mut sb = Sandbox::new("teams");
    sb.write(".mirador/roles/planner.md", "---\nmodel: opus\n---\nYou plan.\n");
    sb.write(".mirador/roles/reviewer.md", "---\nmodel: haiku\n---\nYou review.\n");
    sb.write(
        ".mirador/teams/squad.md",
        "---\nmembers:\n  - role: planner\n    lead: true\n  - role: reviewer\n    count: 2\n---\nShip small.\n",
    );
    sb.launch();
    let first = sb.first_pane();

    let tab = sb.call(Request::TeamStart {
        team: "squad".into(),
        task: Some("plan the chart".into()),
        pane_id: Some(first.clone()),
    })["tabId"]
        .as_str()
        .unwrap()
        .to_string();

    // One tab, three members, each its own agent; the task to the lead only.
    let names: Vec<String> = sb.wait_for("the team to list", LONG, || {
        let t = sb.teams().into_iter().find(|t| t["tabId"] == tab.as_str())?;
        Some(t["members"].as_array()?.iter().map(|m| m["name"].as_str().unwrap().to_string()).collect())
    });
    assert_eq!(names, ["planner", "reviewer-1", "reviewer-2"]);
    let planner = sb.nth_start(&pane_of(&sb, "planner"), 1);
    assert_eq!(planner.last().map(String::as_str), Some("plan the chart"), "{planner:?}");
    let r2 = sb.nth_start(&pane_of(&sb, "reviewer-2"), 1);
    assert_eq!(cmux_e2e::flag(&r2, "--name").as_deref(), Some("reviewer-2"));
    assert_ne!(r2.last().map(String::as_str), Some("plan the chart"), "only the lead gets the task");
    let prompt = std::fs::read_to_string(cmux_e2e::flag(&r2, "--append-system-prompt-file").unwrap()).unwrap();
    assert!(prompt.starts_with("You review.\n\n# Your team: squad"), "{prompt}");
    assert!(prompt.contains("- `reviewer-1` (reviewer)") && prompt.contains("Ship small."), "{prompt}");
    for name in ["planner", "reviewer-1", "reviewer-2"] {
        sb.wait_for(&format!("{name} running"), LONG, || state(&sb, name).filter(|s| s == "running"));
    }

    // One exits to its shell, another's pane is closed.
    let r1 = pane_of(&sb, "reviewer-1");
    sb.type_into(&r1, "/exit\r");
    sb.wait_for("reviewer-1 exited", LONG, || state(&sb, "reviewer-1").filter(|s| s == "exited"));
    sb.call(Request::ClosePane { pane_id: pane_of(&sb, "reviewer-2") });
    sb.wait_for("reviewer-2 closed", LONG, || state(&sb, "reviewer-2").filter(|s| s == "closed"));

    // A restart never types into a shell that is running something.
    sb.type_into(&r1, "sleep 307\r");
    sb.wait_for("the sleep to run", LONG, || cmux_e2e::process_running("^sleep 307$").then_some(()));
    let outcome = |done: &serde_json::Value, name: &str| {
        done["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["name"] == name)
            .and_then(|m| m["outcome"].as_str().map(String::from))
    };
    let done = sb.call(Request::TeamRestart { members: vec![], pane_id: Some(pane_of(&sb, "planner")) });
    assert_eq!(outcome(&done, "planner").as_deref(), Some("running"));
    assert_eq!(outcome(&done, "reviewer-1").as_deref(), Some("busy"));
    assert_eq!(outcome(&done, "reviewer-2").as_deref(), Some("reopened"));
    assert_eq!(sb.events("start", &r1).len(), 1, "nothing was typed into the busy shell");
    sb.nth_start(&pane_of(&sb, "reviewer-2"), 1);

    // Idle again, it is restarted in its own pane.
    sb.type_into(&r1, "\x03");
    sb.wait_for("the sleep to end", LONG, || (!cmux_e2e::process_running("^sleep 307$")).then_some(()));
    let done = sb.call(Request::TeamRestart { members: vec!["reviewer-1".into()], pane_id: Some(r1.clone()) });
    assert_eq!(outcome(&done, "reviewer-1").as_deref(), Some("restarted"));
    let again = sb.nth_start(&r1, 2);
    assert_eq!(cmux_e2e::flag(&again, "--name").as_deref(), Some("reviewer-1"));

    // A new member is numbered after its role's.
    sb.call(Request::TeamAdd { role: "reviewer".into(), name: None, pane_id: Some(r1.clone()) });
    let r3 = sb.wait_for("reviewer-3", LONG, || sb.member("reviewer-3"));
    sb.nth_start(r3["pane"].as_str().unwrap(), 1);

    // Quit and relaunch: still the team, in the same tab, every member.
    let before: Vec<(String, String)> = sb.teams()[0]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| (m["name"].as_str().unwrap().into(), m["pane"].as_str().unwrap().into()))
        .collect();
    sb.quit();
    sb.launch();
    let teams = sb.teams();
    assert_eq!(teams.len(), 1);
    assert_eq!(teams[0]["tabId"], tab.as_str());
    let after: Vec<(String, String)> = teams[0]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| (m["name"].as_str().unwrap().into(), m["pane"].as_str().unwrap().into()))
        .collect();
    assert_eq!(after, before);
}
