//! A remote computer, end to end: the sandbox's own sshd, reached as `box`
//! from an ssh pane. Skipped unless `MIRA_E2E=1`.

// The remote is a Unix sshd sandbox reached over Unix sockets, and the
// Windows side of remotes is not done: none of this builds there.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use cmux_e2e::{e2e, flag, Sandbox};
use cmux_protocol::Request;

const LONG: Duration = Duration::from_secs(45);

/// The app's version, which every crate of the workspace shares: the
/// `mira` a remote must run.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where a remote agent's files go: `<home>/.mirador/run/<pane>`, then
/// `.sh`, `.prompt.md`, `.settings.json`.
fn run_base(sb: &Sandbox, pane: &str) -> String {
    format!("{}/.mirador/run/{pane}", sb.home.display())
}

/// Asserts the settings file at `path` gives the agent Mirador's hooks,
/// calling the remote's `mira`.
fn assert_hooks(path: &str, mira: &std::path::Path) {
    let settings: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        settings["hooks"]["SessionStart"][0]["hooks"][0]["command"],
        format!("\"{}\" claude-hook session-start", mira.display()),
        "{settings}"
    );
    assert_eq!(settings["hooks"]["Stop"][0]["hooks"][0]["command"], format!("\"{}\" claude-hook stop", mira.display()));
    assert!(settings.get("statusLine").is_none(), "the status line is the person's own: {settings}");
}

/// An ssh pane to `box`, in a tab of its own (a split would be narrow
/// enough to wrap a path), once its shell is up: the pane's master
/// connection is what the app's ssh requests ride on.
fn remote_pane(sb: &Sandbox) -> String {
    let pane = sb.call(Request::SshOpen { host: "box".into(), target: Some("tab".into()) })["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    // The login shell is the passwd entry's: zsh on this Mac (`host% `),
    // bash on CI's runner (`host:dir user$ `), after macOS's banner urging
    // zsh. The prompt is the last line written.
    sb.wait_for_screen(&pane, "the remote shell's prompt", LONG, |s| {
        s.lines().map(str::trim_end).rfind(|l| !l.is_empty()).is_some_and(|l| l.ends_with('%') || l.ends_with('$'))
    });
    pane
}

/// `box` is in `ssh hosts`, an ssh pane reaches it, and the login there
/// looks like the sandbox: its home, its `bin` first on the PATH, and no
/// `MIRA_PANE`, which ssh does not carry.
#[test]
fn an_ssh_pane_reaches_the_sandboxs_own_remote() {
    e2e!();
    let mut sb = Sandbox::new("remote");
    sb.sshd();
    sb.launch();
    sb.first_pane();

    let (code, hosts, err) = sb.mira(None, &["ssh", "hosts"]);
    assert!(code == 0 && hosts.lines().any(|l| l == "box"), "{hosts}{err}");

    let pane = remote_pane(&sb);
    sb.type_into(&pane, "echo home=$HOME; echo \"pane=[$MIRA_PANE]\"; echo ssh=$SSH_CONNECTION; command -v claude\r");
    let home = format!("home={}", sb.home.display());
    let claude = format!("{}/claude", sb.root.join("bin").display());
    let screen = sb.wait_for_screen(&pane, "the remote's answers", LONG, |s| {
        s.lines().any(|l| l.trim_end() == claude)
    });
    let lines: Vec<&str> = screen.lines().map(str::trim_end).collect();
    assert!(lines.contains(&home.as_str()), "{screen}");
    assert!(lines.contains(&"pane=[]"), "{screen}");
    assert!(lines.iter().any(|l| l.starts_with("ssh=127.0.0.1 ")), "over ssh, not a local shell: {screen}");
}

/// A `mira` on the remote has no discovery file there, so it finds no app
/// — until the app forwards its socket over the pane's connection, and
/// `MIRA_SOCKET` names it: then it is this app that answers.
#[test]
fn mira_on_the_remote_reaches_the_app_through_its_forwarded_socket() {
    e2e!();
    let mut sb = Sandbox::new("forward");
    sb.sshd();
    sb.launch();
    sb.first_pane();
    let pane = remote_pane(&sb);

    sb.type_into(&pane, "mira list-tabs\r");
    sb.wait_for_screen(&pane, "mira finding no app", LONG, |s| s.contains("Mirador is not running"));

    let forwarded = sb.call(Request::SshForwardApp { pane_id: Some(pane.clone()) });
    assert_eq!(forwarded["paneId"], pane.as_str());
    let socket = forwarded["socket"].as_str().unwrap().to_string();
    assert!(socket.ends_with(".sock") && socket.len() < 104, "{socket}");
    sb.type_into(&pane, &format!("MIRA_SOCKET={socket} mira list-tabs\r"));
    let tabs: Vec<String> = sb.snapshot()["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(tabs.len(), 2, "the first tab and the ssh pane's");
    sb.wait_for_screen(&pane, "the app's tabs, listed on the remote", LONG, |s| tabs.iter().all(|t| s.contains(t)));

    // The app relaunched while the connection lives on (it outlives the
    // app; the sandbox ends it only at the end): no record of the forward,
    // which the master still holds, so it is cancelled, removed and made
    // again — and the socket is still served, at the same path, which a
    // forward redone over the same master without the cancel would not be.
    sb.quit();
    sb.launch();
    let again = sb.call(Request::SshForwardApp { pane_id: Some(pane.clone()) });
    assert_eq!(again["socket"], socket.as_str());
    assert!(std::os::unix::net::UnixStream::connect(&socket).is_ok(), "nothing serves {socket} after the relaunch");
}

/// A remote that has the `mira` of this version already keeps it: the app
/// answers its path, and it runs there.
#[test]
fn a_remote_keeps_the_mira_it_has() {
    e2e!();
    let mut sb = Sandbox::new("mirakeep");
    sb.sshd();
    let placed = sb.place_remote_mira(VERSION);
    sb.launch();
    sb.first_pane();
    let pane = remote_pane(&sb);

    let answer = sb.call(Request::SshMira { pane_id: Some(pane.clone()) });
    assert_eq!(answer["paneId"], pane.as_str());
    assert_eq!(answer["mira"], placed.to_str().unwrap());
    sb.type_into(&pane, &format!("{} --version\r", placed.display()));
    sb.wait_for_screen(&pane, "the remote's mira version", LONG, |s| s.lines().any(|l| l.trim_end() == format!("mira {VERSION}")));
}

/// None is published for macOS, which this remote is: the app says so,
/// naming the system and where a person may put one by hand. The fetch
/// itself, on Linux, is never run here: no test touches the network.
#[test]
fn a_remote_with_no_published_mira_says_where_to_put_one() {
    e2e!();
    let mut sb = Sandbox::new("miranone");
    sb.sshd();
    sb.launch();
    sb.first_pane();
    let pane = remote_pane(&sb);

    let err = sb.request(Request::SshMira { pane_id: Some(pane) }).expect_err("no mira for macOS");
    let machine = std::process::Command::new("uname").arg("-m").output().unwrap().stdout;
    let machine = String::from_utf8_lossy(&machine).trim().to_string();
    assert!(err.contains(&format!("no mira is published for darwin/{machine}")), "{err}");
    let by_hand = sb.home.join(format!(".mirador/bin/mira-{VERSION}"));
    assert!(err.contains(by_hand.to_str().unwrap()), "{err}");
}

/// A file sent to the remote arrives byte for byte, owner-only, its
/// directory made: a quote, `$HOME`, a backslash, a line equal to the
/// delimiter its script uses, and no final newline. The remote is this
/// computer, so the file is read straight off the disk.
#[test]
fn a_file_sent_to_the_remote_arrives_exact() {
    e2e!();
    let mut sb = Sandbox::new("put");
    sb.sshd();
    sb.launch();
    sb.first_pane();
    let pane = remote_pane(&sb);

    let path = sb.home.join(".mirador/sent/probe.txt");
    let text = "it's $HOME \\ here\nMIRA_EOF\nEOF\nno newline at the end";
    let answer = sb.call(Request::SshPut {
        pane_id: Some(pane.clone()),
        path: path.to_str().unwrap().into(),
        text: text.into(),
    });
    assert_eq!(answer["paneId"], pane.as_str());
    assert_eq!(std::fs::read(&path).unwrap(), text.as_bytes());
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    assert!(!path.with_extension("txt.tmp").exists(), "nothing left beside it");
}

/// `agent new --host box --dir <dir>`: plain Claude Code on the remote, in
/// that directory, told its pane and this app's socket, given Mirador's
/// hooks, listed as an agent on `box` — and a message sent from a local
/// pane reaches it. Codex there is not done yet, and a host goes with a
/// directory.
#[test]
fn a_plain_agent_runs_on_the_remote() {
    e2e!();
    let mut sb = Sandbox::new("ragent");
    sb.sshd();
    let mira = sb.place_remote_mira(VERSION);
    sb.launch();
    let first = sb.first_pane();
    let dir = sb.project.canonicalize().unwrap().to_str().unwrap().to_string();
    let agent_new = |role: Option<&str>, harness: Option<cmux_protocol::Harness>| Request::AgentNew {
        role: role.map(String::from),
        harness,
        title: None,
        task: Some("look around".into()),
        target: None,
        pane_id: Some(first.clone()),
        host: Some("box".into()),
        dir: Some(dir.clone()),
    };

    let (code, _, err) = sb.mira(Some(&first), &["agent", "new", "--host", "box", "look around"]);
    assert!(code != 0 && err.contains("--dir"), "{err}");
    let err = sb.request(agent_new(None, Some(cmux_protocol::Harness::Codex))).expect_err("not on a remote yet");
    assert!(err.contains("not done yet"), "{err}");
    assert_eq!(sb.panes().len(), 1, "a refusal opens no pane");

    let pane = sb.call(agent_new(None, None))["paneId"].as_str().unwrap().to_string();
    let start = sb.wait_for("the remote agent's start", LONG, || sb.events("start", &pane).into_iter().next());
    assert_eq!(start["kind"], "claude");
    assert_eq!(start["cwd"], dir.as_str());
    // Even a plain agent has settings there: Mirador's hooks.
    let settings = format!("{}.settings.json", run_base(&sb, &pane));
    assert_eq!(start["args"], serde_json::json!(["--settings", settings, "look around"]));
    assert_hooks(&settings, &mira);
    let socket = start["socket"].as_str().unwrap_or_default();
    assert!(socket.ends_with(".sock") && std::path::Path::new(socket).exists(), "{start}");

    let agents = sb.call(Request::AgentList)["agents"].as_array().cloned().unwrap();
    let listed = agents.iter().find(|a| a["paneId"] == pane.as_str()).unwrap_or_else(|| sb.fail("not in agent list"));
    assert_eq!((listed["harness"].as_str(), listed["remoteHost"].as_str()), (Some("claude"), Some("box")), "{listed}");

    sb.call(Request::AgentSend {
        to: pane.clone(),
        message: "hello from here".into(),
        pane_id: Some(first.clone()),
        force: false,
        card: None,
        title: None,
    });
    sb.wait_for("the message on the remote", LONG, || {
        sb.events("message", &pane).into_iter().find(|m| m["text"].as_str().is_some_and(|t| t.contains("hello from here")))
    });
}

/// The pid of the ssh this computer runs for `pane`'s attach, if any. The
/// remote is this computer too, so its side of the attach is in the same
/// process table: `mira attach <pane>`, and the login shell that ran it,
/// given the line with the pane id still quoted. Only the pane's own ssh
/// has `ssh` and `-tt` before that quoted id. The pattern starts at `ssh`
/// for a second reason: pgrep takes one that starts with `-` as an option.
fn attach_ssh(pane: &str) -> Option<String> {
    let out = std::process::Command::new("pgrep").args(["-f", &format!("ssh .*-tt .*attach '{pane}'")]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().next().map(String::from)
}

/// A remote agent runs under a holder there, and its pane only attaches to
/// it: the connection dropped, the pane says so and the agent runs on,
/// started once. The pane reattaches by itself, the agent's screen comes
/// back from the holder, and it takes messages and reaches the app again.
#[test]
fn a_remote_agent_outlives_its_connection() {
    e2e!();
    let mut sb = Sandbox::new("rhold");
    sb.sshd();
    sb.place_remote_mira(VERSION);
    sb.launch();
    let first = sb.first_pane();
    let dir = sb.project.canonicalize().unwrap().to_str().unwrap().to_string();
    let pane = sb.call(Request::AgentNew {
        role: None,
        harness: None,
        title: None,
        task: Some("look around".into()),
        target: None,
        pane_id: Some(first.clone()),
        host: Some("box".into()),
        dir: Some(dir),
    })["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    let start = sb.wait_for("the remote agent's start", LONG, || sb.events("start", &pane).into_iter().next());
    // The remote's runtime directory is the sandbox's (sshd's SetEnv).
    let holder = cmux_core::hold::conn::endpoint(&sb.runtime.join("mirador").join("holders"), &pane);
    assert!(cmux_core::hold::conn::is_alive(&holder), "no holder on the remote at {}", holder.display());
    sb.wait_for_screen(&pane, "the agent, through the attach", LONG, |s| s.contains("❯"));
    let socket = start["socket"].as_str().unwrap_or_default().to_string();
    let first_ssh = attach_ssh(&pane).unwrap_or_else(|| sb.fail("no ssh for the pane's attach"));

    // Drop the connection: its master goes, and every session over it.
    let dropped = std::process::Command::new(sb.root.join("bin/ssh"))
        .args(["-o", &format!("ControlPath={}/mirador/ssh/cm-%C", sb.runtime.display()), "-O", "exit", "box"])
        .env("HOME", &sb.home)
        .output()
        .unwrap();
    assert!(dropped.status.success(), "ssh -O exit: {}", String::from_utf8_lossy(&dropped.stderr));
    sb.wait_for_screen(&pane, "the pane to say it lost the connection", LONG, |s| {
        s.contains("[disconnected]") && s.contains("reconnecting in 2s")
    });

    assert!(cmux_core::hold::conn::is_alive(&holder), "the holder went with the connection");
    let pid = start["pid"].to_string();
    let alive = std::process::Command::new("kill").args(["-0", &pid]).status().unwrap();
    assert!(alive.success(), "the agent ({pid}) went with the connection");

    // Back by itself: a new ssh, the agent's screen from the holder's
    // replay, the app's socket forwarded again, and still one agent.
    sb.wait_for("the pane's ssh to reattach", LONG, || attach_ssh(&pane).filter(|p| *p != first_ssh));
    sb.wait_for_screen(&pane, "the agent back in the pane", LONG, |s| match s.rsplit_once("reconnecting in") {
        Some((_, after)) => after.contains("❯"),
        // The replay may clear the screen first.
        None => s.contains("❯"),
    });
    sb.wait_for("the app's socket forwarded again", LONG, || std::os::unix::net::UnixStream::connect(&socket).ok());
    sb.call(Request::AgentSend {
        to: pane.clone(),
        message: "hello after the drop".into(),
        pane_id: Some(first.clone()),
        force: false,
        card: None,
        title: None,
    });
    sb.wait_for("the message after the drop", LONG, || {
        sb.events("message", &pane).into_iter().find(|m| m["text"].as_str().is_some_and(|t| t.contains("hello after the drop")))
    });
    assert_eq!(sb.events("start", &pane).len(), 1, "the agent started more than once");
}

/// A role from this computer's catalog runs on the remote: its model, its
/// prompt and its settings — sent there, Mirador's hooks in them — and it
/// reports: the hooks it runs reach this app through the forwarded
/// socket, and its pane gets a status. A role that keeps only some skills
/// is not done on a remote yet: listing them runs Claude Code here.
#[test]
fn a_role_runs_on_the_remote_and_reports_through_its_hooks() {
    e2e!();
    let mut sb = Sandbox::new("rrole");
    sb.sshd();
    let mira = sb.place_remote_mira(VERSION);
    sb.write(".mirador/roles/reviewer.md", "---\nmodel: haiku\n---\nYou review.\n");
    sb.write(".mirador/roles/charts.md", "---\nskills: [pdf]\n---\nCharts.\n");
    sb.launch();
    let first = sb.first_pane();
    let dir = sb.project.canonicalize().unwrap().to_str().unwrap().to_string();
    let agent_new = |role: &str| Request::AgentNew {
        role: Some(role.into()),
        harness: None,
        title: None,
        task: Some("look around".into()),
        target: None,
        pane_id: Some(first.clone()),
        host: Some("box".into()),
        dir: Some(dir.clone()),
    };

    let err = sb.request(agent_new("charts")).expect_err("a skills list is not done on a remote");
    assert!(err.contains("not done on a remote yet"), "{err}");
    assert_eq!(sb.panes().len(), 1, "a refusal opens no pane");

    let pane = sb.call(agent_new("reviewer"))["paneId"].as_str().unwrap().to_string();
    let args = sb.nth_start(&pane, 1);
    let base = run_base(&sb, &pane);
    assert_eq!(flag(&args, "--model").as_deref(), Some("haiku"), "{args:?}");
    let prompt = format!("{base}.prompt.md");
    let settings = format!("{base}.settings.json");
    assert_eq!(flag(&args, "--append-system-prompt-file"), Some(prompt.clone()), "{args:?}");
    assert_eq!(flag(&args, "--settings"), Some(settings.clone()), "{args:?}");
    assert_eq!(args.last().map(String::as_str), Some("look around"));
    assert!(std::fs::read_to_string(&prompt).unwrap().contains("You review."));
    assert_hooks(&settings, &mira);

    let listed = sb.wait_for("the remote agent's status, from its hooks", LONG, || {
        let agents = sb.call(Request::AgentList)["agents"].as_array().cloned().unwrap_or_default();
        agents.into_iter().find(|a| a["paneId"] == pane.as_str() && !a["status"].is_null())
    });
    assert_eq!((listed["role"].as_str(), listed["remoteHost"].as_str()), (Some("reviewer"), Some("box")), "{listed}");
    let hooked = sb.events("hooked", &pane);
    assert_eq!(hooked.first().map(|h| h["ok"].clone()), Some(serde_json::json!(true)), "{hooked:?}");
}

/// `team start --host box --dir <dir>`: every member on the remote, the
/// team and its roles still this computer's. Each starts in the directory
/// there, the lead with the task; each is told its teammates and calls
/// `mira` by its path there, never this machine's; each reports through
/// its hooks; a message from a local pane reaches one. A team with a Codex
/// member opens nothing, naming the member; a restart is not done yet.
#[test]
fn a_team_runs_on_the_remote() {
    e2e!();
    let mut sb = Sandbox::new("rteam");
    sb.sshd();
    let mira = sb.place_remote_mira(VERSION);
    sb.write(".mirador/roles/planner.md", "---\nmodel: opus\n---\nYou plan.\n");
    sb.write(".mirador/roles/reviewer.md", "---\nmodel: haiku\n---\nYou review.\n");
    sb.write(".mirador/roles/coder.md", "---\nharness: codex\n---\nYou code.\n");
    sb.write(".mirador/teams/squad.md", "---\nmembers:\n  - role: planner\n    lead: true\n  - role: reviewer\n---\nShip small.\n");
    sb.write(".mirador/teams/mixed.md", "---\nmembers:\n  - role: planner\n    lead: true\n  - role: coder\n---\nMixed.\n");
    sb.launch();
    let first = sb.first_pane();
    let dir = sb.project.canonicalize().unwrap().to_str().unwrap().to_string();
    let team_start = |team: &str| Request::TeamStart {
        team: team.into(),
        task: Some("plan the chart".into()),
        pane_id: Some(first.clone()),
        title: None,
        host: Some("box".into()),
        dir: Some(dir.clone()),
    };

    let err = sb.request(team_start("mixed")).expect_err("Codex is not done on a remote");
    assert!(err.contains("member `coder`") && err.contains("not done yet"), "{err}");
    assert_eq!(sb.snapshot()["tabs"].as_array().unwrap().len(), 1, "a team that cannot start opens nothing");

    sb.call(team_start("squad"));
    sb.wait_for("the team to list", LONG, || {
        let names: Vec<String> = sb.teams().first()?["members"].as_array()?.iter().map(|m| m["name"].as_str().unwrap().to_string()).collect();
        (names == ["planner", "reviewer"]).then_some(())
    });
    let (planner, reviewer) = (pane_of(&sb, "planner"), pane_of(&sb, "reviewer"));
    for pane in [&planner, &reviewer] {
        let start = sb.wait_for("a member's start", LONG, || sb.events("start", pane).into_iter().next());
        assert_eq!(start["cwd"], dir.as_str(), "{start}");
    }
    let lead = sb.nth_start(&planner, 1);
    assert_eq!(lead.last().map(String::as_str), Some("plan the chart"), "{lead:?}");
    assert_ne!(sb.nth_start(&reviewer, 1).last().map(String::as_str), Some("plan the chart"), "only the lead gets the task");
    let prompt = std::fs::read_to_string(flag(&lead, "--append-system-prompt-file").unwrap()).unwrap();
    assert!(prompt.starts_with("You plan.\n\n# Your team: squad"), "{prompt}");
    assert!(prompt.contains("- `reviewer` (reviewer)") && prompt.contains(mira.to_str().unwrap()), "{prompt}");
    assert!(!prompt.contains("target/debug/mira"), "never this machine's mira: {prompt}");

    sb.wait_for("both members' status, from their hooks", LONG, || {
        let agents = sb.call(Request::AgentList)["agents"].as_array().cloned().unwrap_or_default();
        let reporting = |pane: &str| agents.iter().any(|a| a["paneId"] == pane && a["remoteHost"] == "box" && !a["status"].is_null());
        (reporting(&planner) && reporting(&reviewer)).then_some(())
    });

    sb.call(Request::AgentSend {
        to: reviewer.clone(),
        message: "hello from here".into(),
        pane_id: Some(first.clone()),
        force: false,
        card: None,
        title: None,
    });
    sb.wait_for("the message on the remote", LONG, || {
        sb.events("message", &reviewer).into_iter().find(|m| m["text"].as_str().is_some_and(|t| t.contains("hello from here")))
    });

    let err = sb.request(Request::TeamRestart { members: vec![], pane_id: Some(planner.clone()) }).expect_err("not on a remote yet");
    assert!(err.contains("not done yet"), "{err}");
    let err = sb
        .request(Request::TeamAdd { role: "reviewer".into(), name: None, pane_id: Some(planner), task: None, title: None })
        .expect_err("not on a remote yet");
    assert!(err.contains("adding a member there is not done yet"), "{err}");
    // A role on a remote asked for in a team's tab would be a member there,
    // or an agent outside the team: neither is done yet.
    let err = sb
        .request(Request::AgentNew {
            role: Some("reviewer".into()),
            harness: None,
            title: None,
            task: None,
            target: None,
            pane_id: Some(reviewer.clone()),
            host: Some("box".into()),
            dir: Some(dir.clone()),
        })
        .expect_err("not in a team's tab yet");
    assert!(err.contains("in a team's tab is not done yet"), "{err}");
}

fn pane_of(sb: &Sandbox, name: &str) -> String {
    sb.member(name).and_then(|m| m["pane"].as_str().map(String::from)).expect(name)
}
