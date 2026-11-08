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
