//! An agent's trail, end to end: what it did, read from its own transcript
//! through `mira agent trail`. The fake `claude` writes a transcript in
//! Claude Code's shape and names it with its hooks, as Claude Code does;
//! the fake `codex` holds a rollout open, as Codex does. Skipped unless
//! `MIRA_E2E=1`.

use std::time::Duration;

use cmux_e2e::{e2e, Sandbox};
use cmux_protocol::Request;

const LONG: Duration = Duration::from_secs(45);

#[test]
fn an_agent_s_trail_shows_what_it_did() {
    e2e!();
    let mut sb = Sandbox::new("trail");
    sb.set_env("E2E_TURN_MS", "500");
    sb.write(".mirador/roles/dev.md", "---\nmodel: opus\n---\nYou build.\n");
    sb.write(".mirador/roles/coder.md", "---\nharness: codex\nmodel: gpt-5\n---\nYou code.\n");
    sb.write(
        ".mirador/teams/pair.md",
        "---\nmembers:\n  - role: dev\n    lead: true\n  - role: coder\n---\nShip small.\n",
    );
    sb.launch();
    let shell = sb.first_pane();
    sb.call(Request::TeamStart { team: "pair".into(), task: None, pane_id: Some(shell.clone()) });
    sb.wait_for("the dev", LONG, || sb.member("dev"));
    let dev = sb.member("dev").and_then(|m| m["pane"].as_str().map(String::from)).unwrap();
    sb.wait_for("the dev's session", LONG, || sb.events("hooked", &dev).into_iter().next());
    let coder = sb.member("coder").and_then(|m| m["pane"].as_str().map(String::from)).unwrap();

    // Before its first turn there is nothing to show, and that is no error.
    let (code, out, err) = sb.mira(Some(&shell), &["agent", "trail", "--pane", &dev]);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with('('), "{out}");

    let (code, _, err) = sb.mira(Some(&shell), &["agent", "send", &dev, "--wait", "--timeout", "40", "trail-check please"]);
    assert_eq!(code, 0, "{err}");

    // The turn's prompt, then its one action: a Bash call that worked,
    // with its id for --detail. By name from a teammate, as `agent send`
    // finds it.
    let (code, out, err) = sb.mira(Some(&coder), &["agent", "trail", "dev"]);
    assert_eq!(code, 0, "{err}");
    let action = out.lines().find(|l| l.contains("Bash")).unwrap_or_else(|| panic!("no Bash action in:\n{out}"));
    assert!(out.contains("trail-check please"), "the turn's prompt:\n{out}");
    assert!(action.contains("✓") && action.contains("echo trail-check please"), "{action}");
    let id = action.rsplit_once('[').and_then(|(_, rest)| rest.strip_suffix(']')).expect("an id");

    // Its input and output in full, by that id.
    let (code, detail, err) = sb.mira(Some(&shell), &["agent", "trail", "--pane", &dev, "--detail", id]);
    assert_eq!(code, 0, "{err}");
    assert!(detail.starts_with(&format!("Bash  {id}")), "{detail}");
    let output = detail.split_once("--- output\n").map(|(_, o)| o).expect("an output section");
    assert!(output.starts_with("trail-check please"), "{detail}");
    let (code, _, err) = sb.mira(Some(&coder), &["agent", "trail", "dev", "--detail", "toolu_none"]);
    assert_ne!(code, 0);
    assert!(err.contains("no action toolu_none"), "{err}");

    // A command that fails: ✗ with its exit code, its detail an error.
    let (code, _, err) = sb.mira(Some(&shell), &["agent", "send", &dev, "--wait", "--timeout", "40", "build it !fail"]);
    assert_eq!(code, 0, "{err}");
    let out = sb.mira(Some(&shell), &["agent", "trail", "--pane", &dev]).1;
    let failed = out.lines().find(|l| l.contains("build it") && l.contains('[')).unwrap_or_else(|| panic!("no failed action in:\n{out}"));
    assert!(failed.contains("✗") && failed.contains("(exit 1)"), "{failed}");
    let id = failed.rsplit_once('[').and_then(|(_, rest)| rest.strip_suffix(']')).expect("an id");
    let detail = sb.mira(Some(&shell), &["agent", "trail", "--pane", &dev, "--detail", id]).1;
    assert!(detail.contains("--- output (error)\nExit code 1"), "{detail}");

    // A Codex agent's trail is the rollout it holds open.
    let trail = |pane: &str| sb.mira(Some(&shell), &["agent", "trail", "--pane", pane]).1;
    let shows = |pane: &str, what: &str| sb.wait_for(what, LONG, || Some(trail(pane)).filter(|t| t.contains(what)));
    sb.mira(Some(&shell), &["agent", "send", &coder, "codex-check"]);
    let first = shows(&coder, "echo codex-check");
    assert!(first.contains("✓ CommandExecution"), "{first}");

    // `/new` starts another rollout in the same process: the trail follows.
    sb.mira(Some(&shell), &["agent", "send", &coder, "/new"]);
    sb.wait_for("the new session", LONG, || sb.events("message", &coder).iter().any(|m| m["text"] == "/new").then_some(()));
    sb.mira(Some(&shell), &["agent", "send", &coder, "after-new"]);
    let second = shows(&coder, "echo after-new");
    assert!(!second.contains("codex-check"), "the old session's turn:\n{second}");
    let id = second.lines().find(|l| l.contains("after-new") && l.contains('[')).and_then(|l| l.rsplit_once('['))
        .and_then(|(_, rest)| rest.strip_suffix(']')).expect("an id").to_string();

    // Both agents exit; what they last wrote stays their pane's trail,
    // and each action's detail with it, once the pane is no agent's.
    for pane in [&coder, &dev] {
        sb.mira(Some(&shell), &["agent", "send", pane, "/exit"]);
        sb.wait_for("the agent to exit", LONG, || sb.events("exit", pane).into_iter().next());
        sb.wait_for("the pane to be no agent's", LONG, || {
            let agents = sb.call(Request::AgentList);
            (!agents["agents"].as_array().unwrap().iter().any(|a| a["paneId"] == pane.as_str())).then_some(())
        });
    }
    assert!(trail(&coder).contains("echo after-new"), "{}", trail(&coder));
    assert!(trail(&dev).contains("echo trail-check please"), "{}", trail(&dev));
    let (code, detail, err) = sb.mira(Some(&shell), &["agent", "trail", "--pane", &coder, "--detail", &id]);
    assert_eq!(code, 0, "{err}");
    assert!(detail.contains("--- output\nafter-new"), "{detail}");
}
