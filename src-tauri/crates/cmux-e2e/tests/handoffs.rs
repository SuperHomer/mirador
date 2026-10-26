//! Handoffs, end to end: one agent typing work into another, and waiting
//! for it (#120). Through the real `mira` CLI, as an agent would run it:
//! `wait` and its exit codes live there. Skipped unless `MIRA_E2E=1`.

use std::time::Duration;

use cmux_e2e::{e2e, Sandbox};
use cmux_protocol::Request;

const LONG: Duration = Duration::from_secs(45);

fn pane_of(sb: &Sandbox, name: &str) -> String {
    sb.member(name).and_then(|m| m["pane"].as_str().map(String::from)).expect(name)
}

fn send(sb: &Sandbox, from: &str, to: &str, message: &str) -> Result<serde_json::Value, String> {
    sb.request(Request::AgentSend { title: None,
        to: to.into(),
        message: message.into(),
        pane_id: Some(from.into()),
        force: false,
        card: None,
    })
}

#[test]
fn agents_hand_each_other_work() {
    e2e!();
    let mut sb = Sandbox::new("handoff");
    sb.set_env("E2E_TURN_MS", "1500");
    sb.write(".mirador/roles/planner.md", "---\nmodel: opus\n---\nYou plan.\n");
    sb.write(".mirador/roles/reviewer.md", "---\nmodel: haiku\n---\nYou review.\n");
    sb.write(
        ".mirador/teams/squad.md",
        "---\nmembers:\n  - role: planner\n    lead: true\n  - role: reviewer\n    count: 2\n---\nShip small.\n",
    );
    sb.launch();
    let shell = sb.first_pane();
    sb.call(Request::TeamStart { title: None, team: "squad".into(), task: None, pane_id: Some(shell.clone()) });
    sb.wait_for("the planner", LONG, || sb.member("planner"));
    let planner = pane_of(&sb, "planner");
    let r1 = pane_of(&sb, "reviewer-1");
    let r2 = pane_of(&sb, "reviewer-2");
    for pane in [&planner, &r1, &r2] {
        sb.wait_for("each member's session", LONG, || sb.events("hooked", pane).into_iter().next());
    }

    // reviewer-1 hands the planner a message of several lines, and waits
    // for the turn it starts to end.
    let (code, _, err) = sb.mira(
        Some(&r1),
        &["agent", "send", "planner", "--wait", "--timeout", "40", "Review done.\n1. tests\n2. naming"],
    );
    assert_eq!(code, 0, "send --wait: {err}");
    assert!(err.contains("planner finished"), "{err}");
    let got = sb.events("message", &planner);
    assert_eq!(
        got.last().and_then(|m| m["text"].as_str()),
        Some("[message from reviewer-1, via mira agent send]\nReview done.\n1. tests\n2. naming"),
        "one message, every line, headed with who sent it"
    );
    assert_eq!(sb.events("turn-done", &planner).len(), 1, "the wait ended on the turn's stop");

    // What must never be typed into.
    let ambiguous = send(&sb, &r1, "reviewer", "hi").unwrap_err();
    assert!(ambiguous.contains("reviewer-1, reviewer-2"), "{ambiguous}");
    let shell_err = send(&sb, &r1, &shell, "touch /tmp/should-not-exist").unwrap_err();
    assert!(shell_err.contains("runs no agent"), "{shell_err}");
    let me = send(&sb, &r1, "reviewer-1", "hi").unwrap_err();
    assert!(me.contains("cannot message itself"), "{me}");
    let elsewhere = send(&sb, &shell, "planner", "hi").unwrap_err();
    assert!(elsewhere.contains("no agent `planner` in this tab"), "{elsewhere}");

    // A permission prompt: a wait begun before it ends with 2, and a
    // message is refused until forced.
    let waiter = {
        let mira = sb.mira_command(Some(&r1), &["agent", "wait", "reviewer-2", "--timeout", "40"]);
        std::thread::spawn(move || {
            let mut mira = mira;
            mira.output().map(|o| o.status.code())
        })
    };
    sb.wait_for("the wait to begin", LONG, || {
        cmux_e2e::process_running("mira agent wait reviewer-2 --timeout 40$").then_some(())
    });
    send(&sb, &r1, "reviewer-2", "please !ask before you push").unwrap();
    let code = waiter.join().unwrap().unwrap();
    assert_eq!(code, Some(2), "a wait ends with 2 on a permission prompt");
    let refused = send(&sb, &r1, "reviewer-2", "are you there?").unwrap_err();
    assert!(refused.contains("waiting on a person"), "{refused}");
    sb.request(Request::AgentSend { title: None,
        to: "reviewer-2".into(),
        message: "yes, go ahead".into(),
        pane_id: Some(r1.clone()),
        force: true,
        card: None,
    })
    .unwrap();
    sb.wait_for("the forced message", LONG, || {
        sb.events("message", &r2).into_iter().find(|m| m["text"].as_str().is_some_and(|t| t.ends_with("yes, go ahead")))
    });

    // Nothing happening: a wait gives up with 124.
    let (code, _, _) = sb.mira(Some(&r1), &["agent", "wait", "planner", "--timeout", "2"]);
    assert_eq!(code, 124);
    assert!(!std::path::Path::new("/tmp/should-not-exist").exists());
}
