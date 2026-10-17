//! The app, end to end. Skipped unless `MIRA_E2E=1` (see the crate docs
//! for what to build first).

use std::time::Duration;

use cmux_e2e::{e2e, Sandbox};
use cmux_protocol::Request;
use serde_json::Value;

const LONG: Duration = Duration::from_secs(45);

/// The first thing that breaks when the app does not start at all: a
/// render throw, a panic at startup, a frontend that never mounted, a PTY
/// that never spawned. A pane that echoes proves all four.
#[test]
fn a_pane_mounts_and_its_terminal_echoes() {
    e2e!();
    let mut sb = Sandbox::new("echo");
    sb.launch();
    let panes = sb.panes();
    assert_eq!(panes.len(), 1, "a fresh sandbox opens one pane: {panes:?}");
    let pane = &panes[0];

    // `read-screen` answers only once the pane's terminal is drawn: a
    // render throw shows as this never happening.
    sb.wait_for("the pane to mount", LONG, || sb.screen(pane));
    // The arithmetic is the shell's: the line typed says `$((6*7))`, so
    // only the shell's answer prints `e2e-42`.
    sb.type_into(pane, "echo e2e-$((6*7))\r");
    sb.wait_for("the shell's answer", LONG, || {
        sb.screen(pane).filter(|s| s.lines().any(|l| l.trim() == "e2e-42"))
    });
    // And it started where the config said.
    let tab = &sb.snapshot()["tabs"][0];
    let cwd = tab["cwd"].as_str().unwrap_or_default().to_string();
    assert!(cwd.ends_with("proj"), "the first pane works in defaultCwd, not {cwd:?}");
}

/// A role's Claude Code pane, quit and relaunched, comes back as the same
/// pane resuming the same conversation — on the role's model, and still
/// held to the role's limits, which a resumed conversation does not keep
/// by itself (#118).
#[test]
fn a_role_pane_comes_back_resumed_with_its_model_and_limits() {
    e2e!();
    let mut sb = Sandbox::new("restore");
    std::fs::create_dir_all(sb.project.join(".mirador/roles")).unwrap();
    std::fs::write(
        sb.project.join(".mirador/roles/reviewer.md"),
        "---\nmodel: haiku\nmcp: []\ntools:\n  deny: [\"Bash(git push *)\"]\n---\nYou review. Never edit.\n",
    )
    .unwrap();
    sb.launch();
    let first = sb.panes()[0].clone();
    sb.wait_for("the first pane to mount", LONG, || sb.screen(&first));

    let pane = sb.call(Request::AgentNew {
        role: Some("reviewer".into()),
        task: Some("look at main".into()),
        target: None,
        pane_id: Some(first.clone()),
    })["paneId"]
        .as_str()
        .unwrap()
        .to_string();

    // The fake claude starts in that pane, with the role's flags…
    let started = sb.wait_for("the reviewer's claude to start", LONG, || {
        sb.agent_events()
            .into_iter()
            .find(|e| e["event"] == "start" && e["pane"] == pane.as_str())
    });
    let args: Vec<String> = serde_json::from_value(started["args"].clone()).unwrap();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    assert_eq!(flag("--model").as_deref(), Some("haiku"), "{args:?}");
    assert_eq!(flag("--name").as_deref(), Some("reviewer"), "{args:?}");
    assert_eq!(args.last().map(String::as_str), Some("look at main"), "{args:?}");
    let prompt = flag("--append-system-prompt-file").expect("a prompt file");
    assert!(std::fs::read_to_string(&prompt).unwrap().contains("You review."));
    let settings = flag("--settings").expect("a settings file, for its limits");
    let held: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(held["allowedMcpServers"], serde_json::json!([]));
    assert_eq!(held["permissions"]["deny"], serde_json::json!(["Bash(git push *)"]));

    // …and records its session, as Claude Code's hooks would.
    let session = sb.wait_for("the session hook", LONG, || {
        sb.agent_events()
            .into_iter()
            .find(|e| e["event"] == "hooked" && e["pane"] == pane.as_str() && e["ok"] == true)
            .and_then(|e| e["session"].as_str().map(String::from))
    });
    let before = sb.panes();

    sb.quit();
    sb.launch();

    // The same panes, and the reviewer's typed back in: resuming its
    // session, on its model, under the same limits.
    assert_eq!(sb.panes(), before, "the layout comes back as it was");
    let resumed = sb.wait_for("the reviewer to resume", LONG, || {
        sb.agent_events().into_iter().find(|e| {
            e["event"] == "start"
                && e["pane"] == pane.as_str()
                && e["args"].as_array().is_some_and(|a| a.iter().any(|x| x == "--resume"))
        })
    });
    let args: Vec<String> = serde_json::from_value(resumed["args"].clone()).unwrap();
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    assert_eq!(flag("--resume").as_deref(), Some(session.as_str()), "{args:?}");
    assert_eq!(flag("--model").as_deref(), Some("haiku"), "{args:?}");
    assert_eq!(flag("--settings").as_deref(), Some(settings.as_str()), "{args:?}");
    // A resumed conversation keeps its prompt; giving it again would
    // append it twice (#117).
    assert_eq!(flag("--append-system-prompt-file"), None, "{args:?}");
    assert_eq!(sb.launches(), 2);
}

/// The editor for role, way and team files opens beside the pane it is
/// asked from, knowing that pane's directory, and is still an editor after
/// a relaunch. What it does inside is the webview's, out of reach here;
/// `role_files`' tests cover what it may read and write.
#[test]
fn the_agent_editor_opens_and_comes_back() {
    e2e!();
    let mut sb = Sandbox::new("editor");
    sb.launch();
    let first = sb.first_pane();
    let (code, out, err) = sb.mira(Some(&first), &["agent", "edit"]);
    assert_eq!(code, 0, "{err}");
    let editor = out.trim().to_string();
    let listed = |sb: &Sandbox| -> Option<Value> {
        sb.snapshot()["agentEditorPanes"]
            .as_array()?
            .iter()
            .find(|e| e["paneId"] == editor.as_str())
            .cloned()
    };
    let pane = sb.wait_for("the editor pane", LONG, || listed(&sb));
    assert!(pane["cwd"].as_str().is_some_and(|c| c.ends_with("proj")), "{pane}");
    assert_eq!(sb.panes(), [first.clone(), editor.clone()]);
    // A render throw in the editor takes the whole tree down with it: the
    // terminal beside it still answering says the page survived the editor.
    sb.type_into(&first, "echo still-$((40+2))\r");
    sb.wait_for("the terminal beside the editor", LONG, || {
        sb.screen(&first).filter(|s| s.lines().any(|l| l.trim() == "still-42"))
    });

    sb.quit();
    sb.launch();
    assert!(listed(&sb).is_some(), "the editor comes back as an editor, not a shell");
}
