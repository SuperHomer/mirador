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

/// A message sent the moment an agent is started — `mira agent new` then
/// `mira agent send`, as a lead does — waits for the agent to be running.
/// Typed at once, it went to the pane's shell, still to type the agent's
/// command in, and ran there as a command (CI, #159).
#[test]
fn a_message_sent_before_its_agent_is_up_waits_for_it() {
    e2e!();
    let mut sb = Sandbox::new("sendearly");
    // A slow shell: its terminal runs for seconds before the agent's
    // command is typed in, as on a loaded machine.
    std::fs::write(sb.home.join(".zshrc"), "sleep 3\n").unwrap();
    sb.launch();
    let first = sb.first_pane();
    let pane = sb.call(Request::AgentNew {
        role: None,
        harness: None,
        title: None,
        task: None,
        target: None,
        pane_id: Some(first.clone()),
    })["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    // Its terminal running, its shell still in `.zshrc`: where CI sent.
    sb.type_into(&pane, "");
    send(&sb, &first, &pane, "hello early").unwrap();
    sb.wait_for("the message, to the agent", LONG, || {
        sb.events("message", &pane).into_iter().find(|m| m["text"] == "hello early")
    });
    let screen = sb.screen(&pane).unwrap_or_default();
    assert!(!screen.contains("command not found"), "the shell ran it:\n{screen}");
}

/// An agent showing a dialog no hook reports — Codex's update prompt, an
/// approval, Claude Code's own questions — is not typed into: the Enter
/// after a message, or after `/clear`, would pick the selected option. In
/// a live run it accepted Codex's "Update now".
#[test]
fn an_agent_showing_a_menu_is_not_typed_into() {
    e2e!();
    let mut sb = Sandbox::new("sendmenu");
    sb.launch();
    let first = sb.first_pane();
    let pane = sb.call(Request::AgentNew {
        role: None,
        harness: None,
        title: None,
        task: None,
        target: None,
        pane_id: Some(first.clone()),
    })["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    sb.wait_for("its session", LONG, || sb.events("hooked", &pane).into_iter().next());
    send(&sb, &first, &pane, "!menu").unwrap();
    sb.wait_for("the menu", LONG, || sb.screen(&pane).filter(|s| s.contains("❯ 1. Yes")));
    let before = sb.events("message", &pane).len();

    let refused = send(&sb, &first, &pane, "go on").unwrap_err();
    assert!(refused.contains("showing a choice") && refused.contains("`❯ 1. Yes`"), "{refused}");
    let clear = |force| sb.request(Request::AgentClear { name: Some(pane.clone()), pane_id: Some(first.clone()), force });
    let refused = clear(false).unwrap_err();
    assert!(refused.contains("showing a choice") && refused.contains("`❯ 1. Yes`"), "{refused}");

    // Forced, each goes through — and is all the agent ever got.
    sb.request(Request::AgentSend {
        title: None,
        to: pane.clone(),
        message: "yes, go ahead".into(),
        pane_id: Some(first.clone()),
        force: true,
        card: None,
    })
    .unwrap();
    clear(true).unwrap();
    let got: Vec<String> = sb.wait_for("the forced two", LONG, || {
        let texts: Vec<String> = sb.events("message", &pane)[before..]
            .iter()
            .map(|m| m["text"].as_str().unwrap_or_default().to_string())
            .collect();
        (texts.len() >= 2).then_some(texts)
    });
    assert_eq!(got, ["yes, go ahead", "/clear"]);
}

/// An agent with no input box yet is not typed into: Codex without a
/// login shows its logo and waits for a key, and a message sent then
/// dismissed it, its Enter landing on the login menu that replaced it.
/// Only once the input box is up does a message go in as one.
#[test]
fn an_agent_with_no_input_box_is_not_typed_into() {
    e2e!();
    let mut sb = Sandbox::new("sendwelcome");
    sb.set_env("E2E_WELCOME", "1");
    sb.launch();
    let first = sb.first_pane();
    let pane = sb.call(Request::AgentNew {
        role: None,
        harness: None,
        title: None,
        task: None,
        target: None,
        pane_id: Some(first.clone()),
    })["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    sb.wait_for("its session", LONG, || sb.events("hooked", &pane).into_iter().next());
    sb.wait_for("the welcome", LONG, || sb.screen(&pane).filter(|s| s.contains("Press any key")));

    let asked = std::time::Instant::now();
    let refused = send(&sb, &first, &pane, "hello").unwrap_err();
    let took = asked.elapsed();
    assert!(refused.contains("no input box"), "{refused}");
    // The wait is 10 s, and no read of the screen outlives it.
    assert!(took >= Duration::from_secs(10) && took < Duration::from_secs(12), "refused after {took:?}");
    let clear = |force| sb.request(Request::AgentClear { name: Some(pane.clone()), pane_id: Some(first.clone()), force });
    let refused = clear(false).unwrap_err();
    assert!(refused.contains("no input box"), "{refused}");
    assert_eq!(sb.events("message", &pane).len(), 0, "nothing reached it");

    // Forced, it goes through — and, as a key would, dismisses the welcome.
    sb.request(Request::AgentSend {
        title: None,
        to: pane.clone(),
        message: "hello anyway".into(),
        pane_id: Some(first.clone()),
        force: true,
        card: None,
    })
    .unwrap();
    sb.wait_for("the forced message", LONG, || {
        sb.events("message", &pane).into_iter().find(|m| m["text"] == "hello anyway")
    });
    // Its input box up, a message goes in unforced.
    send(&sb, &first, &pane, "now for real").unwrap();
    sb.wait_for("the message", LONG, || sb.events("message", &pane).into_iter().find(|m| m["text"] == "now for real"));
}
