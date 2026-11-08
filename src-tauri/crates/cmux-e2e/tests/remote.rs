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
