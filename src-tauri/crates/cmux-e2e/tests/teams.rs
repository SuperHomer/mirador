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

    let tab = sb.call(Request::TeamStart { title: None,
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

/// The team that ships with Mirador starts in a sandbox with no role, way
/// or team file at all: a planner and a worker on Claude Code's Opus 5.5,
/// the planner unable to edit, and a reviewer on Codex — each told the
/// omakase way of working.
#[test]
fn the_built_in_omakase_team_starts_out_of_the_box() {
    e2e!();
    let mut sb = Sandbox::new("omakase");
    sb.launch();
    let first = sb.first_pane();
    sb.call(Request::TeamStart { title: None,
        team: "omakase".into(),
        task: Some("add a health check".into()),
        pane_id: Some(first),
    });
    let names: Vec<String> = sb.wait_for("the team", LONG, || {
        let t = sb.teams().into_iter().next()?;
        Some(t["members"].as_array()?.iter().map(|m| m["name"].as_str().unwrap().to_string()).collect())
    });
    assert_eq!(names, ["planner", "worker", "reviewer"]);

    let planner = sb.nth_start(&pane_of(&sb, "planner"), 1);
    assert_eq!(cmux_e2e::flag(&planner, "--model").as_deref(), Some("claude-opus-5-5"));
    assert_eq!(planner.last().map(String::as_str), Some("add a health check"));
    let settings: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(cmux_e2e::flag(&planner, "--settings").expect("the planner's limits")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["permissions"]["deny"], serde_json::json!(["Edit", "Write", "NotebookEdit"]));
    let prompt = std::fs::read_to_string(cmux_e2e::flag(&planner, "--append-system-prompt-file").unwrap()).unwrap();
    assert!(prompt.contains("You are the planner") && prompt.contains("# The omakase way"), "{prompt}");

    let worker = sb.nth_start(&pane_of(&sb, "worker"), 1);
    assert_eq!(cmux_e2e::flag(&worker, "--model").as_deref(), Some("claude-opus-5-5"));
    assert_eq!(cmux_e2e::flag(&worker, "--settings"), None, "the worker is not limited");

    // The reviewer is Codex: its prompt is its developer_instructions.
    let reviewer_pane = pane_of(&sb, "reviewer");
    let start = sb.wait_for("the reviewer", LONG, || sb.events("start", &reviewer_pane).into_iter().next());
    assert_eq!(start["kind"], "codex");
    let args: Vec<String> = serde_json::from_value(start["args"].clone()).unwrap();
    assert_eq!(cmux_e2e::flag(&args, "--model").as_deref(), Some("gpt-6-luna"));
    let config: toml::Table = toml::from_str(&cmux_e2e::flag(&args, "-c").unwrap()).unwrap();
    let instructions = config["developer_instructions"].as_str().unwrap();
    assert!(instructions.contains("You are the reviewer") && instructions.contains("APPROVE"), "{instructions}");
    assert!(instructions.contains("`planner` (omakase-planner, leads the team)"), "{instructions}");

    // Every member goes by its member name — the Codex one too, which has
    // no session name and once showed as its role, `omakase-reviewer`.
    let agents = sb.call(Request::AgentList)["agents"].as_array().cloned().unwrap();
    let member_of = |pane: &str| {
        agents
            .iter()
            .find(|a| a["paneId"] == pane)
            .and_then(|a| a["member"].as_str().map(String::from))
    };
    assert_eq!(member_of(&reviewer_pane).as_deref(), Some("reviewer"));
    assert_eq!(member_of(&pane_of(&sb, "worker")).as_deref(), Some("worker"));
    let (_, listed, _) = sb.mira(None, &["agent", "list"]);
    let reviewer_line = listed.lines().find(|l| l.starts_with(&reviewer_pane)).unwrap_or_default().to_string();
    assert!(reviewer_line.contains(" reviewer ") && !reviewer_line.contains("omakase-reviewer"), "{listed}");
}

/// Clearing types each agent's own new-conversation command into it, as a
/// person typing /clear does: Claude Code's `/clear`, Codex's `/new`. A
/// team is cleared teammates first and the lead last, and one waiting on
/// a person is left alone.
#[test]
fn a_team_is_cleared_with_its_lead_last() {
    e2e!();
    let mut sb = Sandbox::new("clear");
    sb.write(".mirador/roles/planner.md", "---\nmodel: opus\n---\nYou plan.\n");
    sb.write(".mirador/roles/reviewer.md", "---\nmodel: haiku\n---\nYou review.\n");
    sb.write(".mirador/roles/coder.md", "---\nharness: codex\n---\nYou code.\n");
    sb.write(
        ".mirador/teams/squad.md",
        "---\nmembers:\n  - role: planner\n    lead: true\n  - role: reviewer\n    count: 2\n  - role: coder\n---\nShip small.\n",
    );
    sb.launch();
    let first = sb.first_pane();
    sb.call(Request::TeamStart { title: None, team: "squad".into(), task: None, pane_id: Some(first) });
    sb.wait_for("the team", LONG, || sb.member("coder"));
    let [planner, r1, r2, coder] = ["planner", "reviewer-1", "reviewer-2", "coder"].map(|n| pane_of(&sb, n));
    for pane in [&planner, &r1, &r2] {
        sb.wait_for("each Claude member's session", LONG, || sb.events("hooked", pane).into_iter().next());
    }
    sb.nth_start(&coder, 1);
    for name in ["planner", "reviewer-1", "reviewer-2", "coder"] {
        sb.wait_for(&format!("{name} running"), LONG, || state(&sb, name).filter(|s| s == "running"));
    }
    let cleared = |pane: &str| {
        sb.events("message", pane).iter().filter(|m| matches!(m["text"].as_str(), Some("/clear" | "/new"))).count()
    };

    // One, by name, from a teammate.
    let (code, _, err) = sb.mira(Some(&planner), &["agent", "clear", "reviewer-1"]);
    assert_eq!(code, 0, "agent clear: {err}");
    sb.wait_for("reviewer-1's /clear", LONG, || (cleared(&r1) == 1).then_some(()));
    // The fake takes it as a turn; a line typed during one waits for it,
    // which would put reviewer-1 after the lead below.
    sb.wait_for("that turn to end", LONG, || sb.events("turn-done", &r1).into_iter().next());

    // The whole team, from the lead: reviewer-2 is waiting on a person.
    sb.call(Request::AgentSend {
        title: None,
        to: "reviewer-2".into(),
        message: "!ask".into(),
        pane_id: Some(planner.clone()),
        force: false,
        card: None,
    });
    sb.wait_for("reviewer-2 to need you", LONG, || {
        sb.member("reviewer-2").filter(|m| m["status"] == "needsYou")
    });
    let (code, out, err) = sb.mira(Some(&planner), &["--json", "team", "clear"]);
    assert_eq!(code, 0, "team clear: {err}");
    let done: serde_json::Value = serde_json::from_str(&out).unwrap();
    let outcomes: Vec<(String, String)> = done["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| (m["name"].as_str().unwrap().into(), m["outcome"].as_str().unwrap().into()))
        .collect();
    let want = [("reviewer-1", "cleared"), ("reviewer-2", "needs you"), ("coder", "cleared"), ("planner", "cleared")];
    assert_eq!(outcomes, want.map(|(n, o)| (n.to_string(), o.to_string())), "teammates first, the lead last");
    sb.wait_for("every /clear and /new", LONG, || {
        (cleared(&r1) == 2 && cleared(&coder) == 1 && cleared(&planner) == 1).then_some(())
    });
    assert_eq!(cleared(&r2), 0, "nothing typed into a permission prompt");
    let coder_line = sb.events("message", &coder).last().and_then(|m| m["text"].as_str().map(String::from));
    assert_eq!(coder_line.as_deref(), Some("/new"), "Codex starts over with /new");
    let order: Vec<String> = sb
        .agent_events()
        .into_iter()
        .filter(|e| e["event"] == "message" && matches!(e["text"].as_str(), Some("/clear" | "/new")))
        .map(|e| e["pane"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(order.last(), Some(&planner), "the lead is cleared last: {order:?}");
}

/// A team saved, its tab closed, and resumed: a new tab with the same
/// members, each Claude started again on its own conversation. Never while
/// that conversation still runs somewhere, and the same for one agent.
#[test]
fn a_saved_team_resumes_its_conversations() {
    e2e!();
    let mut sb = Sandbox::new("saves");
    sb.write(".mirador/roles/planner.md", "---\nmodel: opus\n---\nYou plan.\n");
    sb.write(".mirador/roles/reviewer.md", "---\nmodel: haiku\n---\nYou review.\n");
    sb.write(
        ".mirador/teams/squad.md",
        "---\nmembers:\n  - role: planner\n    lead: true\n  - role: reviewer\n    count: 2\n---\nShip small.\n",
    );
    sb.launch();
    let first = sb.first_pane();
    let tab = sb.call(Request::TeamStart { title: None, team: "squad".into(), task: None, pane_id: Some(first.clone()) })
        ["tabId"]
        .as_str()
        .unwrap()
        .to_string();
    sb.wait_for("the team", LONG, || sb.member("reviewer-2"));
    let names = ["planner", "reviewer-1", "reviewer-2"];
    let sessions: Vec<String> = names
        .iter()
        .map(|n| {
            let pane = pane_of(&sb, n);
            let hooked = sb.wait_for("each member's session", LONG, || sb.events("hooked", &pane).into_iter().next());
            hooked["session"].as_str().unwrap().to_string()
        })
        .collect();
    let planner = pane_of(&sb, "planner");
    sb.wait_for("the planner's session recorded", LONG, || {
        let (code, _, _) = sb.mira(Some(&planner), &["team", "save"]);
        (code == 0).then_some(())
    });
    let (code, out, err) = sb.mira(Some(&planner), &["agent", "save", "reviewer-1", "--as", "solo"]);
    assert_eq!(code, 0, "agent save: {err}");
    assert!(out.contains("saved solo: reviewer-1"), "{out}");
    let (_, listed, _) = sb.mira(None, &["team", "saves"]);
    assert!(listed.lines().next().is_some_and(|l| l.starts_with("solo ")), "newest first: {listed}");
    assert!(listed.contains("team squad") && listed.contains("planner, reviewer-1, reviewer-2"), "{listed}");

    // Its conversations still run: nothing opens.
    let refused = sb.request(Request::TeamResume { name: "squad".into() }).unwrap_err();
    assert!(refused.contains("still running") && refused.contains(&pane_of(&sb, "planner")), "{refused}");
    let wrong = sb.request(Request::AgentResume { name: "squad".into(), pane_id: None, tab: false }).unwrap_err();
    assert!(wrong.contains("mira team resume squad"), "{wrong}");

    // Ended: one agent comes back beside the first pane, on its own.
    sb.call(Request::CloseTab { tab_id: tab.clone() });
    sb.wait_for("the team gone", LONG, || sb.teams().is_empty().then_some(()));
    let solo = sb.call(Request::AgentResume { name: "solo".into(), pane_id: Some(first.clone()), tab: false })["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    let args = sb.nth_start(&solo, 1);
    assert_eq!(cmux_e2e::flag(&args, "--resume").as_deref(), Some(sessions[1].as_str()), "{args:?}");
    assert_eq!(cmux_e2e::flag(&args, "--model").as_deref(), Some("haiku"));
    sb.wait_for("its session again", LONG, || sb.events("hooked", &solo).into_iter().next());
    let refused = sb.request(Request::TeamResume { name: "squad".into() }).unwrap_err();
    assert!(refused.contains("reviewer-1's in pane") && refused.contains(&solo) && !refused.contains("planner's"), "{refused}");
    sb.call(Request::ClosePane { pane_id: solo });

    // The team, in a tab of its own, every member on its conversation.
    let resumed = sb.wait_for("the team resumed", LONG, || {
        sb.request(Request::TeamResume { name: "squad".into() }).ok().and_then(|v| v["tabId"].as_str().map(String::from))
    });
    assert_ne!(resumed, tab);
    let teams = sb.teams();
    assert_eq!(teams.len(), 1);
    assert_eq!(teams[0]["tabId"], resumed.as_str());
    for (name, session) in names.iter().zip(&sessions) {
        let pane = pane_of(&sb, name);
        let args = sb.nth_start(&pane, 1);
        assert_eq!(cmux_e2e::flag(&args, "--resume").as_deref(), Some(session.as_str()), "{name}: {args:?}");
        assert_eq!(cmux_e2e::flag(&args, "--name").as_deref(), Some(*name));
        let prompt = std::fs::read_to_string(cmux_e2e::flag(&args, "--append-system-prompt-file").unwrap()).unwrap();
        assert!(prompt.contains("# Your team: squad"), "{name}'s prompt passed again: {prompt}");
        sb.wait_for(&format!("{name} running"), LONG, || state(&sb, name).filter(|s| s == "running"));
    }
    let lead = teams[0]["members"].as_array().unwrap().iter().find(|m| m["lead"] == true).map(|m| m["name"].clone());
    assert_eq!(lead, Some("planner".into()));
}
