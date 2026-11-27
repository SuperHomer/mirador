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

/// Output the app strips before the terminal sees it is not output the
/// terminal owes an ack for. 3 MB of OSC 99 with an empty payload — a
/// notification sequence the scanner drops, and that raises nothing —
/// is more than the 2 MB a pane may have unacked. Counted, it parked the
/// reader for good: the pane froze with the shell blocked on its write,
/// and nothing typed afterwards ever showed.
#[test]
fn a_pane_keeps_running_through_output_it_never_shows() {
    e2e!();
    let mut sb = Sandbox::new("stripped");
    sb.launch();
    let pane = sb.first_pane();
    // The sums are the shell's, as above: the typed line shows `$((…))`,
    // so only output past the flood prints `flood-42`.
    sb.type_into(
        &pane,
        "yes $'\\e]99;;\\a' | head -n 450000 | tr -d '\\n'; echo; echo flood-$((6*7))\r",
    );
    sb.wait_for_screen(
        &pane,
        "output after 3 MB of stripped notifications",
        LONG,
        |s| s.lines().any(|l| l.trim() == "flood-42"),
    );
    sb.type_into(&pane, "echo alive-$((6*7+1))\r");
    sb.wait_for_screen(&pane, "typing after the flood", LONG, |s| {
        s.lines().any(|l| l.trim() == "alive-43")
    });
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
        host: None,
        dir: None,
        on: None,
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
    // Its prompt too: a conversation keeps the prompt it started with only
    // until it is compacted, then builds it from what this launch passed
    // (Claude Code's --system-prompt-snapshot), and before that a prompt
    // passed again is not sent, so it cannot be doubled.
    assert_eq!(flag("--append-system-prompt-file").as_deref(), Some(prompt.as_str()), "{args:?}");
    assert_eq!(sb.launches(), 2);

    // A prompt file gone since is left out, not the resume.
    std::fs::remove_file(&prompt).unwrap();
    sb.quit();
    sb.launch();
    let args = sb.nth_start(&pane, 3);
    assert_eq!(cmux_e2e::flag(&args, "--resume").as_deref(), Some(session.as_str()), "{args:?}");
    assert_eq!(cmux_e2e::flag(&args, "--append-system-prompt-file"), None, "{args:?}");
}

/// The Codex thread the session file holds for a pane, once it is saved.
fn saved_codex_thread(sb: &Sandbox) -> Option<String> {
    let file = ["Library/Application Support/Mirador", ".local/share/mirador"]
        .iter()
        .find_map(|d| std::fs::read_to_string(sb.home.join(d).join("session.json")).ok())?;
    let at = file.find("\"codex:")? + "\"codex:".len();
    Some(file[at..at + 36].to_string())
}

/// Where nothing can name the rollout a Codex holds open — Windows, which
/// lists no process's files — the `notify` Mirador gives every Codex
/// records its thread after a turn. It also reports the turn's end, as
/// Claude Code's `stop` hook does: the pane idle, and its notification.
/// The person's own notify, from their Codex config, still runs, with the
/// same JSON.
#[test]
fn a_codex_reports_its_turn_through_notify_and_keeps_the_persons_own() {
    e2e!();
    let mut sb = Sandbox::new("codexnotify");
    sb.set_env("E2E_CODEX_NO_ROLLOUT", "1");
    let marker = sb.root.join("their-notify.json");
    std::fs::create_dir_all(sb.home.join(".codex")).unwrap();
    std::fs::write(
        sb.home.join(".codex/config.toml"),
        format!("notify = [\"/bin/sh\", \"-c\", \"printf '%s' \\\"$1\\\" > '{}'\", \"sh\"]\n", marker.display()),
    )
    .unwrap();
    sb.launch();
    let first = sb.first_pane();
    let pane = sb.call(Request::AgentNew {
        role: None,
        harness: Some(cmux_protocol::Harness::Codex),
        title: None,
        task: None,
        target: None,
        pane_id: Some(first.clone()),
        host: None,
        dir: None,
        on: None,
    })["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    let started = sb.wait_for("the codex to start", LONG, || sb.events("start", &pane).into_iter().next());
    let args: Vec<String> = serde_json::from_value(started["args"].clone()).unwrap();
    assert!(args.iter().any(|a| a.starts_with("notify=[") && a.contains("codex-notify")), "{args:?}");
    let thread = format!("{:08x}-0000-4000-8000-{:012x}", started["pid"].as_u64().unwrap(), 1);
    assert_eq!(saved_codex_thread(&sb), None, "nothing to name it before a turn");
    let status = |sb: &Sandbox| {
        let agents = sb.call(Request::AgentList)["agents"].as_array().cloned().unwrap_or_default();
        agents.iter().find(|a| a["paneId"] == pane.as_str()).map(|a| a["status"].clone())
    };
    assert_eq!(status(&sb), Some(Value::Null), "nothing to say before a turn");
    // Looking at another pane, so its notification is not read on arrival.
    sb.call(Request::FocusPane { pane_id: first.clone() });

    let (code, _, err) = sb.mira(Some(&first), &["agent", "send", &pane, "hello"]);
    assert_eq!(code, 0, "{err}");
    let saved = sb.wait_for("the thread in the session file", LONG, || saved_codex_thread(&sb));
    assert_eq!(saved, thread);
    assert_eq!(status(&sb), Some(Value::from("idle")));
    sb.wait_for("the turn's end notified", LONG, || (sb.snapshot()["unreadPanes"] == serde_json::json!([pane])).then_some(()));

    // Every turn's end, not only the first: the status is Idle again,
    // but a wait started before it still sees it.
    let (code, _, err) = sb.mira(Some(&first), &["agent", "send", &pane, "--wait", "--timeout", "20", "again"]);
    assert_eq!(code, 0, "the second turn's end: {err}");

    // A Codex's report never lands in a pane that runs none: its record
    // there would type `codex resume` into a shell on the next launch.
    let refused = sb.request(Request::AgentSession {
        pane_id: Some(first.clone()),
        tty: None,
        agent: "codex".into(),
        session_id: Some(thread.clone()),
        job: None,
        event: Some("stop".into()),
        transcript_path: None,
        notification_type: None,
        message: None,
    });
    assert!(refused.is_err(), "{refused:?}");
    let agents = sb.call(Request::AgentList)["agents"].as_array().cloned().unwrap_or_default();
    assert!(agents.iter().all(|a| a["paneId"] != first.as_str()), "the shell is no agent: {agents:?}");
    let theirs = sb.wait_for("the person's own notify", LONG, || std::fs::read_to_string(&marker).ok().filter(|t| !t.is_empty()));
    let payload: Value = serde_json::from_str(&theirs).unwrap();
    assert_eq!((payload["type"].as_str(), payload["thread-id"].as_str()), (Some("agent-turn-complete"), Some(thread.as_str())));
}

/// A role's Codex pane whose Codex is gone at launch comes back resuming
/// its thread — recorded from the rollout it held open, Codex having no
/// hooks — with the role's flags and Mirador's socket, as it started.
#[test]
fn a_codex_role_pane_comes_back_resuming_its_thread() {
    e2e!();
    let mut sb = Sandbox::new("codexrestore");
    sb.write(".mirador/roles/coder.md", "---\nharness: codex\nmodel: gpt-5\n---\nYou code.\n");
    sb.launch();
    let first = sb.first_pane();
    let pane = sb.call(Request::AgentNew {
        role: Some("coder".into()),
        harness: None,
        title: None,
        task: Some("look at main".into()),
        target: None,
        pane_id: Some(first),
        host: None,
        dir: None,
        on: None,
    })["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    let started = sb.nth_start(&pane, 1);
    assert_eq!(cmux_e2e::flag(&started, "--model").as_deref(), Some("gpt-5"), "{started:?}");
    // The poller finds its rollout and records the thread; the session
    // file says so within a second.
    let thread = sb.wait_for("the coder's thread in the session file", LONG, || saved_codex_thread(&sb));

    // persistSessions is off here: the quit ends the Codex.
    sb.quit();
    sb.launch();

    let args = sb.nth_start(&pane, 2);
    assert_eq!(args[..2], ["resume".to_string(), thread.clone()], "{args:?}");
    assert_eq!(cmux_e2e::flag(&args, "--model").as_deref(), Some("gpt-5"), "{args:?}");
    let given: Vec<&str> = args.windows(2).filter(|w| w[0] == "-c").map(|w| w[1].as_str()).collect();
    assert!(given.iter().any(|c| c.starts_with("developer_instructions=") && c.contains("You code.")), "{args:?}");
    assert!(given.contains(&"default_permissions=\"mirador\""), "{args:?}");
    // Its task was the launch's, not the resume's.
    assert!(!args.iter().any(|a| a == "look at main"), "{args:?}");
}

/// Pi and OpenCode start plain from `mira agent new --harness`, each with
/// the task as its first message the way its CLI takes one — Pi as an
/// argument after `--`, OpenCode's TUI as `--prompt` — and are listed as
/// what they are from the start.
#[test]
fn plain_pi_and_opencode_start_with_their_task() {
    e2e!();
    let mut sb = Sandbox::new("piopencode");
    sb.launch();
    let first = sb.first_pane();
    // Each on a conversation Mirador picked, so it can resume it.
    for (harness, kind, session, task, listed) in [
        ("pi", "pi", "--session-id", ["--", "do x"], "pi"),
        ("opencode", "opencode", "--session", ["--prompt", "do x"], "openCode"),
    ] {
        let (code, out, err) = sb.mira(Some(&first), &["agent", "new", "--harness", harness, "do x"]);
        assert_eq!(code, 0, "{harness}: {err}");
        let pane = out.trim().to_string();
        let started = sb.wait_for(&format!("{kind} to start"), LONG, || sb.events("start", &pane).into_iter().next());
        assert_eq!(started["kind"], kind, "{started}");
        let args: Vec<String> = serde_json::from_value(started["args"].clone()).unwrap();
        assert_eq!((args.len(), args[0].as_str(), &args[2..]), (4, session, &task.map(String::from)[..]), "{args:?}");
        let agents = sb.call(Request::AgentList)["agents"].as_array().cloned().unwrap();
        let agent = agents.iter().find(|a| a["paneId"] == pane.as_str()).unwrap_or_else(|| sb.fail("not in agent list"));
        assert_eq!(agent["harness"], listed, "{agent}");
    }
}

/// A Pi's and an OpenCode's pane come back on the conversation each was
/// started on: Mirador picks the id up front (`--session-id`,
/// `--session`) and types the same launch line, no task, with the same id
/// after a relaunch. A cleared one is not brought back, since `/new` left
/// that conversation. And a team with a Pi member is saved and resumed on
/// its id.
#[test]
fn pi_and_opencode_panes_come_back_on_their_conversation() {
    e2e!();
    let mut sb = Sandbox::new("piocrestore");
    sb.write(".mirador/roles/builder.md", "---\nharness: pi\nmodel: sonnet\n---\nYou build.\n");
    sb.write(".mirador/roles/helper.md", "---\nharness: opencode\n---\nYou help.\n");
    sb.write(".mirador/roles/spare.md", "---\nharness: pi\n---\nYou spare.\n");
    sb.write(".mirador/teams/pair.md", "---\nmembers:\n  - role: builder\n    lead: true\n---\nShip small.\n");
    sb.launch();
    let first = sb.first_pane();
    let start = |role: &str| {
        sb.call(Request::AgentNew {
            role: Some(role.into()),
            harness: None,
            title: None,
            task: Some("look at main".into()),
            target: None,
            pane_id: Some(first.clone()),
            host: None,
            dir: None,
            on: None,
        })["paneId"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let (builder, helper, spare) = (start("builder"), start("helper"), start("spare"));
    let pi = sb.nth_start(&builder, 1);
    let pi_id = cmux_e2e::flag(&pi, "--session-id").expect("a session id");
    assert_eq!(pi.last().map(String::as_str), Some("look at main"), "{pi:?}");
    let oc = sb.nth_start(&helper, 1);
    let oc_id = cmux_e2e::flag(&oc, "--session").expect("a session");
    assert!(oc_id.starts_with("ses_"), "{oc:?}");
    sb.nth_start(&spare, 1);
    // `/new` moves the spare to a conversation Mirador does not know.
    let (code, _, err) = sb.mira(Some(&first), &["agent", "clear", &spare]);
    assert_eq!(code, 0, "{err}");
    sb.wait_for("the spare's /new", LONG, || sb.events("message", &spare).into_iter().find(|m| m["text"] == "/new"));

    sb.quit();
    sb.launch();
    let pi = sb.nth_start(&builder, 2);
    assert_eq!(cmux_e2e::flag(&pi, "--session-id").as_deref(), Some(pi_id.as_str()), "{pi:?}");
    assert_eq!(cmux_e2e::flag(&pi, "--model").as_deref(), Some("sonnet"), "{pi:?}");
    assert!(!pi.iter().any(|a| a == "look at main"), "the task was the launch's: {pi:?}");
    let oc = sb.nth_start(&helper, 2);
    assert_eq!(oc[0], "--standalone", "{oc:?}");
    assert_eq!(cmux_e2e::flag(&oc, "--session").as_deref(), Some(oc_id.as_str()), "{oc:?}");
    assert_eq!(sb.events("start", &spare).len(), 1, "the cleared conversation is not brought back");

    // A team's Pi member, saved and resumed in a tab of its own.
    sb.call(Request::TeamStart { title: None, team: "pair".into(), task: None, pane_id: Some(first), host: None, dir: None, on: None });
    let member = sb.wait_for("the pair's builder", LONG, || sb.member("builder").and_then(|m| m["pane"].as_str().map(String::from)));
    let id = cmux_e2e::flag(&sb.nth_start(&member, 1), "--session-id").expect("a session id");
    let (code, _, err) = sb.mira(Some(&member), &["team", "save"]);
    assert_eq!(code, 0, "{err}");
    let tab = sb.teams()[0]["tabId"].as_str().unwrap().to_string();
    sb.call(Request::CloseTab { tab_id: tab });
    sb.wait_for("the pair gone", LONG, || sb.teams().is_empty().then_some(()));
    sb.call(Request::TeamResume { name: "pair".into() });
    let member = sb.wait_for("the pair resumed", LONG, || sb.member("builder").and_then(|m| m["pane"].as_str().map(String::from)));
    let args = sb.nth_start(&member, 1);
    assert_eq!(cmux_e2e::flag(&args, "--session-id").as_deref(), Some(id.as_str()), "{args:?}");
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

/// The computers are in mirador.json, one file, so there is one computers
/// pane: `computer open` opens it beside the pane it is asked from, asked
/// again from another tab it is focused rather than opened twice, and it
/// comes back after a relaunch. What it draws is the webview's, out of
/// reach here; that it draws without taking the page down is not.
#[test]
fn there_is_one_computers_pane() {
    e2e!();
    let mut sb = Sandbox::new("computers");
    let path = sb.config.join("mirador/mirador.json");
    let mut cfg: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    cfg["computers"] = serde_json::json!([{ "name": "devbox", "host": "box", "dir": "/srv/app" }]);
    std::fs::write(&path, cfg.to_string()).unwrap();
    sb.launch();
    let first = sb.first_pane();
    let (code, out, err) = sb.mira(Some(&first), &["computer", "open", "--split"]);
    assert_eq!(code, 0, "{err}");
    let computers = out.trim().to_string();
    let listed = |sb: &Sandbox| -> Vec<String> {
        sb.snapshot()["computersPanes"]
            .as_array()
            .map(|l| l.iter().filter_map(|c| c["paneId"].as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    assert_eq!(listed(&sb), std::slice::from_ref(&computers));
    assert_eq!(sb.panes(), [first.clone(), computers.clone()]);
    // A render throw in the pane takes the whole tree down with it: the
    // terminal beside it still answering says the page survived it.
    sb.type_into(&first, "echo still-$((40+2))\r");
    sb.wait_for("the terminal beside the computers pane", LONG, || {
        sb.screen(&first).filter(|s| s.lines().any(|l| l.trim() == "still-42"))
    });

    let (code, out, err) = sb.mira(None, &["new-tab"]);
    assert_eq!(code, 0, "{err}");
    let elsewhere = out.trim().to_string();
    let (code, out, err) = sb.mira(Some(&elsewhere), &["computer", "open", "--tab"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.trim(), computers, "the open computers pane again");
    assert_eq!(listed(&sb), std::slice::from_ref(&computers));
    let snap = sb.snapshot();
    let active = snap["tabs"].as_array().unwrap().iter().find(|t| t["id"] == snap["activeTab"]).unwrap().clone();
    assert_eq!(active["focusedPane"], computers.as_str(), "shown, in its own tab");

    sb.quit();
    sb.launch();
    assert_eq!(listed(&sb), [computers], "it comes back as the computers pane, not a shell");
}

/// `mira usage` reads the accounts from the sandbox's own home, the
/// sidebar's Usage section draws them, and `--tab` opens the one usage
/// pane, which draws without taking the page
/// down, is focused again rather than opened twice, and comes back after
/// a relaunch.
#[test]
fn the_usage_pane_opens_once_and_comes_back() {
    e2e!();
    let mut sb = Sandbox::new("usage");
    std::fs::write(
        sb.home.join(".claude.json"),
        r#"{"oauthAccount":{"organizationType":"claude_max","emailAddress":"e2e@example.com"}}"#,
    )
    .unwrap();
    let day = sb.home.join(".codex/sessions/2026/11/02");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(sb.home.join(".codex/auth.json"), r#"{"auth_mode":"chatgpt","OPENAI_API_KEY":null}"#).unwrap();
    let limits = r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":71.0,"window_minutes":10080,"resets_at":4102444800},"secondary":null,"plan_type":"plus"}}}"#;
    std::fs::write(day.join("rollout-2026-11-02T10-00-00-a.jsonl"), format!("{limits}\n")).unwrap();
    sb.launch();
    let first = sb.first_pane();
    // The sidebar's Usage section draws these accounts from the start: a
    // throw in it takes the page down, and the terminal stops answering.
    sb.type_into(&first, "echo sidebar-$((40+2))\r");
    sb.wait_for("the terminal beside the sidebar's usage", LONG, || {
        sb.screen(&first).filter(|s| s.lines().any(|l| l.trim() == "sidebar-42"))
    });

    let (code, out, err) = sb.mira(Some(&first), &["usage", "--json"]);
    assert_eq!(code, 0, "{err}");
    let accounts: Value = serde_json::from_str(&out).unwrap();
    let (claude, codex) = (&accounts["accounts"][0], &accounts["accounts"][1]);
    assert_eq!((&claude["login"], &claude["plan"], &claude["email"]), (&"subscription".into(), &"claude_max".into(), &"e2e@example.com".into()));
    assert_eq!((&codex["login"], &codex["plan"]), (&"subscription".into(), &"plus".into()));
    assert_eq!(codex["limits"][0]["name"], "weekly");

    let (code, out, err) = sb.mira(Some(&first), &["usage", "--tab"]);
    assert_eq!(code, 0, "{err}");
    let usage = out.trim().to_string();
    let listed = |sb: &Sandbox| -> Vec<String> {
        sb.snapshot()["usagePanes"]
            .as_array()
            .map(|l| l.iter().filter_map(|u| u["paneId"].as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    sb.wait_for("the usage pane", LONG, || listed(&sb).contains(&usage).then_some(()));
    assert_eq!(sb.panes(), [first.clone(), usage.clone()]);
    // A render throw in the pane takes the whole tree down with it: the
    // terminal in the other tab still answering says the page survived it.
    sb.type_into(&first, "echo still-$((40+2))\r");
    sb.wait_for("the terminal beside the usage pane", LONG, || {
        sb.screen(&first).filter(|s| s.lines().any(|l| l.trim() == "still-42"))
    });

    let (code, out, err) = sb.mira(Some(&first), &["usage", "--split"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(out.trim(), usage, "the open usage pane again");
    assert_eq!(listed(&sb), std::slice::from_ref(&usage));

    sb.quit();
    sb.launch();
    assert_eq!(listed(&sb), [usage], "it comes back as the usage pane, not a shell");
}

/// A graph pane is still a graph pane after a relaunch, on the same
/// repository: it once came back as a plain shell, its repository never
/// saved in the session.
#[test]
fn a_graph_pane_comes_back_as_a_graph() {
    e2e!();
    let mut sb = Sandbox::new("graphback");
    std::fs::create_dir_all(&sb.project).unwrap();
    std::fs::write(sb.project.join("notes.txt"), "one\n").unwrap();
    for args in [&["init", "-q"][..], &["add", "."], &["commit", "-qm", "init"]] {
        let ok = std::process::Command::new("git")
            .args(["-c", "user.name=e2e", "-c", "user.email=e2e@example.com"])
            .args(args)
            .current_dir(&sb.project)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }
    sb.launch();
    let first = sb.first_pane();
    let (code, out, err) = sb.mira(Some(&first), &["graph", "--tab"]);
    assert_eq!(code, 0, "{err}");
    let graph = out.trim().to_string();
    let listed = |sb: &Sandbox| -> Option<Value> {
        sb.snapshot()["graphPanes"].as_array()?.iter().find(|g| g["paneId"] == graph.as_str()).cloned()
    };
    let before = sb.wait_for("the graph pane", LONG, || listed(&sb));

    sb.quit();
    sb.launch();
    let after = listed(&sb).expect("the graph comes back as a graph, not a shell");
    assert_eq!(after["repo"], before["repo"], "on the same repository");
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
    let agent = sb.call(Request::AgentNew { role: None, harness: None, title: None, task: None, target: None, pane_id: Some(first.clone()), host: None, dir: None, on: None })
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
