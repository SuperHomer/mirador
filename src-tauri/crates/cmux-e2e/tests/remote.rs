//! A remote computer, end to end: the sandbox's own sshd, reached as `box`
//! from an ssh pane. Skipped unless `MIRA_E2E=1`.

use std::time::Duration;

use cmux_e2e::{e2e, Sandbox};
use cmux_protocol::Request;

const LONG: Duration = Duration::from_secs(45);

/// The app's version, which every crate of the workspace shares: the
/// `mira` a remote must run.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// An ssh pane to `box`, in a tab of its own (a split would be narrow
/// enough to wrap a path), once its shell is up: the pane's master
/// connection is what the app's ssh requests ride on.
fn remote_pane(sb: &Sandbox) -> String {
    let pane = sb.call(Request::SshOpen { host: "box".into(), target: Some("tab".into()) })["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    // zsh's prompt: the login shell is the passwd entry's, zsh where this runs.
    sb.wait_for_screen(&pane, "the remote shell's prompt", LONG, |s| s.lines().any(|l| l.trim_end().ends_with('%')));
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert!(!path.with_extension("txt.tmp").exists(), "nothing left beside it");
}

/// `agent new --host box --dir <dir>`: plain Claude Code on the remote, in
/// that directory, told its pane and this app's socket, listed as an agent
/// on `box` — and a message sent from a local pane reaches it. A role or
/// Codex there is not done yet, and a host goes with a directory.
#[test]
fn a_plain_agent_runs_on_the_remote() {
    e2e!();
    let mut sb = Sandbox::new("ragent");
    sb.sshd();
    sb.place_remote_mira(VERSION);
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
    for refused in [agent_new(Some("reviewer"), None), agent_new(None, Some(cmux_protocol::Harness::Codex))] {
        let err = sb.request(refused).expect_err("not on a remote yet");
        assert!(err.contains("not done yet"), "{err}");
    }
    assert_eq!(sb.panes().len(), 1, "a refusal opens no pane");

    let pane = sb.call(agent_new(None, None))["paneId"].as_str().unwrap().to_string();
    let start = sb.wait_for("the remote agent's start", LONG, || sb.events("start", &pane).into_iter().next());
    assert_eq!(start["kind"], "claude");
    assert_eq!(start["cwd"], dir.as_str());
    assert_eq!(start["args"], serde_json::json!(["look around"]));
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
