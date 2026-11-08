//! A remote computer, end to end: the sandbox's own sshd, reached as `box`
//! from an ssh pane. Skipped unless `MIRA_E2E=1`.

use std::time::Duration;

use cmux_e2e::{e2e, Sandbox};
use cmux_protocol::Request;

const LONG: Duration = Duration::from_secs(45);

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

    // A tab of its own: a split would be narrow enough to wrap a path.
    let pane = sb.call(Request::SshOpen { host: "box".into(), target: Some("tab".into()) })["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    // zsh's prompt: the login shell is the passwd entry's, zsh where this runs.
    sb.wait_for_screen(&pane, "the remote shell's prompt", LONG, |s| s.lines().any(|l| l.trim_end().ends_with('%')));
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
    let pane = sb.call(Request::SshOpen { host: "box".into(), target: Some("tab".into()) })["paneId"]
        .as_str()
        .unwrap()
        .to_string();
    // zsh's prompt: the login shell is the passwd entry's, zsh where this runs.
    sb.wait_for_screen(&pane, "the remote shell's prompt", LONG, |s| s.lines().any(|l| l.trim_end().ends_with('%')));

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
