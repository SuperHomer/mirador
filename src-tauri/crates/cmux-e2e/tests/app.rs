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
    // render throw shows as this never happening. Its PTY comes a moment
    // after, and `type_into` waits for that.
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

/// The app's own webview keeps its plugins now that the capability names
/// it, `"webviews": ["main"]`, rather than its window. It learns of a
/// pane added after launch, and answers `read-screen`, through Tauri's
/// event plugin (`plugin:event|listen`, from `core:default`): a pane
/// split off here that then shows its shell says both calls went
/// through — and that the host webview is labelled `main`: named
/// anything else, the capability leaves the app's UI unable to draw even
/// its first pane. The other
/// plugins (notifications, the opener, the updater) are only reached by
/// a person, and no test here sees them.
#[test]
fn the_apps_own_webview_keeps_its_plugins() {
    e2e!();
    let mut sb = Sandbox::new("hostacl");
    sb.launch();
    let first = sb.first_pane();
    let split = sb.call(Request::SplitPane {
        pane_id: Some(first.clone()),
        dir: cmux_protocol::SplitDir::Row,
        command: None,
    })["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    sb.wait_for("the split pane to mount", LONG, || sb.screen(&split));
    sb.type_into(&split, "echo host-$((6*7))\r");
    sb.wait_for("the split pane's shell", LONG, || {
        sb.screen(&split).filter(|s| s.lines().any(|l| l.trim() == "host-42"))
    });
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
        harness: None,
        title: None,
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

/// mirador.json is one file, so there is one settings pane: asked for
/// again, from another tab and whatever the flags, the open one is focused
/// rather than a second opened. It comes back after a relaunch.
#[test]
fn there_is_one_settings_pane() {
    e2e!();
    let mut sb = Sandbox::new("settings");
    sb.launch();
    let first = sb.first_pane();
    let (code, out, err) = sb.mira(Some(&first), &["settings", "--split"]);
    assert_eq!(code, 0, "{err}");
    let settings = out.trim().to_string();
    let listed = |sb: &Sandbox| -> Vec<String> {
        sb.snapshot()["settingsPanes"]
            .as_array()
            .map(|l| l.iter().filter_map(|s| s["paneId"].as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    assert_eq!(listed(&sb), std::slice::from_ref(&settings));
    assert_eq!(sb.panes(), [first.clone(), settings.clone()]);
    // The file it shows, for the keys edited in the file itself.
    let path = sb.config.join("mirador/mirador.json");
    assert_eq!(sb.snapshot()["settingsPanes"][0]["path"], path.to_string_lossy().as_ref());
    // A render throw in the pane takes the whole tree down with it: the
    // terminal beside it still answering says the page survived it.
    sb.type_into(&first, "echo still-$((40+2))\r");
    sb.wait_for("the terminal beside the settings", LONG, || {
        sb.screen(&first).filter(|s| s.lines().any(|l| l.trim() == "still-42"))
    });

    let (code, out, err) = sb.mira(None, &["new-tab"]);
    assert_eq!(code, 0, "{err}");
    let elsewhere = out.trim().to_string();
    let (code, out, err) = sb.mira(Some(&elsewhere), &["settings", "--tab"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.trim(), settings, "the open settings pane again");
    assert_eq!(listed(&sb), std::slice::from_ref(&settings));
    let snap = sb.snapshot();
    let active = snap["tabs"].as_array().unwrap().iter().find(|t| t["id"] == snap["activeTab"]).unwrap().clone();
    assert_eq!(active["focusedPane"], settings.as_str(), "shown, in its own tab");

    sb.quit();
    sb.launch();
    assert_eq!(listed(&sb), [settings], "it comes back as the settings pane, not a shell");
}

/// Opened in a tab, a graph or an editor already open on the same files is
/// shown again rather than opened twice: one graph per repository, one
/// editor per repository or, outside one, per directory — the second ask
/// focusing the first. Another repository gets its own.
#[test]
fn a_graph_or_editor_opened_in_a_tab_is_shown_again_for_the_same_files() {
    e2e!();
    let mut sb = Sandbox::new("reuse");
    let other = sb.home.join("other");
    let plain = sb.home.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    for dir in [&sb.project, &other] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "one\n").unwrap();
        for args in [&["init", "-q"][..], &["add", "."], &["commit", "-qm", "init"]] {
            let ok = std::process::Command::new("git")
                .args(["-c", "user.name=e2e", "-c", "user.email=e2e@example.com"])
                .args(args)
                .current_dir(dir)
                .status()
                .unwrap()
                .success();
            assert!(ok, "git {args:?} in {}", dir.display());
        }
    }
    sb.launch();
    let first = sb.first_pane();
    // A pane in each of the other two directories, each in a tab of its own.
    let pane_in = |dir: &std::path::Path| -> String {
        let (code, out, err) = sb.mira(None, &["new-tab"]);
        assert_eq!(code, 0, "{err}");
        let pane = out.trim().to_string();
        sb.type_into(&pane, &format!("cd {}\r", dir.display()));
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        sb.wait_for("the shell's directory", LONG, || {
            let snap = sb.snapshot();
            let tab = snap["tabs"].as_array()?.iter().find(|t| t["focusedPane"] == pane.as_str())?.clone();
            tab["cwd"].as_str()?.ends_with(&name).then_some(())
        });
        pane
    };
    let in_other = pane_in(&other);
    let in_plain = pane_in(&plain);

    let open = |pane: &str, args: &[&str]| -> String {
        // Somewhere else focused first, so a reuse has to bring it back.
        sb.call(Request::FocusPane { pane_id: first.clone() });
        let (code, out, err) = sb.mira(Some(pane), args);
        assert_eq!(code, 0, "{args:?}: {err}");
        let opened = out.trim().to_string();
        let snap = sb.snapshot();
        let active = snap["tabs"].as_array().unwrap().iter().find(|t| t["id"] == snap["activeTab"]).unwrap().clone();
        assert_eq!(active["focusedPane"], opened.as_str(), "{args:?} shows what it opened");
        opened
    };
    let count = |list: &str| sb.snapshot()[list].as_array().map_or(0, Vec::len);

    let graph = open(&first, &["graph", "--tab"]);
    assert_eq!(open(&first, &["graph", "--tab"]), graph, "the same repository's graph again");
    assert_eq!(count("graphPanes"), 1);
    assert_ne!(open(&in_other, &["graph", "--tab"]), graph, "another repository's");
    assert_eq!(count("graphPanes"), 2);

    let editor = open(&first, &["agent", "edit", "--tab"]);
    assert_eq!(open(&first, &["agent", "edit", "--tab"]), editor, "the same repository's editor again");
    assert_eq!(count("agentEditorPanes"), 1);
    assert_ne!(open(&in_other, &["agent", "edit", "--tab"]), editor);
    let outside = open(&in_plain, &["agent", "edit", "--tab"]);
    assert_eq!(open(&in_plain, &["agent", "edit", "--tab"]), outside, "outside a repository, by its directory");
    assert_eq!(count("agentEditorPanes"), 3);
}

/// `mira agent wall` shows the tab's agents in place of its splits, in the
/// same tab, and again shows the splits. While the wall is up the agent's
/// pane is still in its tab and its terminal, lent to its tile, still
/// takes input and answers `read-screen`; and the wall is still up after a
/// relaunch.
#[test]
fn the_wall_is_a_view_of_its_tab_and_comes_back() {
    e2e!();
    let mut sb = Sandbox::new("wall");
    sb.launch();
    let first = sb.first_pane();
    sb.wait_for("the first pane to mount", LONG, || sb.screen(&first));
    let agent = sb.call(Request::AgentNew { role: None, harness: None, title: None, task: None, target: None, pane_id: Some(first.clone()) })
        ["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    sb.wait_for_screen(&agent, "the fake claude", LONG, |s| s.contains("fake claude ready"));
    sb.wait_for("the agent to be listed", LONG, || {
        let agents = sb.call(Request::AgentList)["agents"].as_array().cloned().unwrap_or_default();
        agents.iter().any(|a| a["paneId"] == agent.as_str()).then_some(())
    });
    let wall = |sb: &Sandbox| -> Vec<bool> {
        let tabs = sb.snapshot()["tabs"].as_array().cloned().unwrap_or_default();
        tabs.iter().map(|t| t["wall"].as_bool().unwrap_or(false)).collect()
    };

    let (code, out, err) = sb.mira(Some(&agent), &["agent", "wall"]);
    assert_eq!((code, out.trim()), (0, "wall on"), "{err}");
    assert_eq!(wall(&sb), [true], "the same tab, now a wall; no tab of its own");
    assert_eq!(sb.panes(), [first.clone(), agent.clone()], "its panes stay");
    sb.type_into(&agent, "through-the-wall\r");
    sb.wait_for_screen(&agent, "the agent's terminal in the wall", LONG, |s| s.contains("through-the-wall"));

    sb.quit();
    sb.launch();
    assert_eq!(wall(&sb), [true], "the wall comes back with its tab");
    assert_eq!(sb.panes(), [first.clone(), agent.clone()]);

    let (code, out, err) = sb.mira(Some(&agent), &["agent", "wall"]);
    assert_eq!((code, out.trim()), (0, "wall off"), "{err}");
    assert_eq!(wall(&sb), [false]);
}
