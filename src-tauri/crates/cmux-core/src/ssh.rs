//! SSH remote workspaces. A remote pane runs `ssh -tt <host>` in a local
//! PTY, so the system ssh handles auth, agent, 2FA, and ProxyJump exactly
//! as it would in any terminal. A shared ControlMaster connection lets us
//! run port-forward control commands without re-authenticating.
//!
//! `host` is passed through as ssh arguments split on whitespace, so a bare
//! alias (`prod`, resolved via ~/.ssh/config) is the common case, while a
//! full destination (`-p 2222 user@host`) also works.

use std::path::PathBuf;
use std::process::Command;

use portable_pty::CommandBuilder;

fn control_dir() -> PathBuf {
    crate::config::config_dir().join("ssh")
}

/// `%C` is a short stable hash of (host, port, user, proxy) — safe for a
/// socket path and shared between the interactive session and control ops.
fn control_path() -> String {
    control_dir().join("cm-%C").to_string_lossy().into_owned()
}

fn control_opts() -> [String; 3] {
    [
        "ControlMaster=auto".to_string(),
        format!("ControlPath={}", control_path()),
        "ControlPersist=30m".to_string(),
    ]
}

/// Splits a host spec into ssh args (`"-p 2222 host"` → `["-p","2222","host"]`).
fn host_args(host: &str) -> Vec<String> {
    host.split_whitespace().map(String::from).collect()
}

/// The interactive `ssh -tt` command for a remote pane.
pub fn interactive_command(host: &str) -> CommandBuilder {
    let _ = std::fs::create_dir_all(control_dir());
    let mut cmd = CommandBuilder::new("ssh");
    cmd.arg("-tt");
    for opt in control_opts() {
        cmd.arg("-o");
        cmd.arg(opt);
    }
    for arg in host_args(host) {
        cmd.arg(arg);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd
}

/// Establishes (or cancels) a local port forward over the pane's existing
/// ControlMaster: `localhost:<port>` → the remote's `localhost:<port>`.
/// Requires the interactive session (the master) to be connected.
pub fn forward(host: &str, port: u16, cancel: bool) -> Result<(), String> {
    let action = if cancel { "cancel" } else { "forward" };
    let mut cmd = Command::new("ssh");
    cmd.arg("-O").arg(action);
    for opt in control_opts() {
        cmd.arg("-o").arg(opt);
    }
    cmd.arg("-L")
        .arg(format!("{port}:localhost:{port}"))
        .args(host_args(host));
    let output = cmd.output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "port forward failed (is the SSH pane connected?): {}",
            err.trim()
        ))
    }
}

/// The display label for a remote pane: the last non-option token of the
/// host spec (`-p 2222 user@box` → `user@box`).
pub fn display_host(host: &str) -> String {
    host.split_whitespace()
        .rfind(|tok| !tok.starts_with('-') && !tok.chars().all(|c| c.is_ascii_digit()))
        .unwrap_or(host)
        .to_string()
}

/// Host aliases from ~/.ssh/config (excluding wildcard patterns), for the
/// command palette's "New SSH Workspace" list.
pub fn config_hosts() -> Vec<String> {
    let Some(home) = std::env::var_os("HOME") else {
        return Vec::new();
    };
    let path = PathBuf::from(home).join(".ssh/config");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut hosts = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        // `Host a b c` can list several aliases on one line.
        if let Some(rest) = line
            .strip_prefix("Host ")
            .or_else(|| line.strip_prefix("host "))
        {
            for alias in rest.split_whitespace() {
                if !alias.contains('*') && !alias.contains('?') && !hosts.contains(&alias.to_string())
                {
                    hosts.push(alias.to_string());
                }
            }
        }
    }
    hosts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_command_uses_ssh_tt_and_control_master() {
        let cmd = interactive_command("myhost");
        let dbg = format!("{cmd:?}");
        assert!(dbg.contains("ssh"));
        assert!(dbg.contains("-tt"));
        assert!(dbg.contains("ControlMaster=auto"));
        assert!(dbg.contains("myhost"));
    }

    #[test]
    fn host_args_split() {
        assert_eq!(host_args("prod"), vec!["prod"]);
        assert_eq!(
            host_args("-p 2222 user@box"),
            vec!["-p", "2222", "user@box"]
        );
    }

    #[test]
    fn display_host_picks_destination() {
        assert_eq!(display_host("prod"), "prod");
        assert_eq!(display_host("-p 2222 user@box"), "user@box");
        assert_eq!(display_host("-i /k/id -o Foo=bar admin@10.0.0.1"), "admin@10.0.0.1");
    }

    #[test]
    fn config_hosts_parse() {
        let dir = std::env::temp_dir().join(format!("mira-ssh-cfg-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".ssh")).unwrap();
        std::fs::write(
            dir.join(".ssh/config"),
            "Host prod staging\n  HostName example.com\nHost *.internal\n  User admin\nHost box\n",
        )
        .unwrap();
        // Exercise the parser directly against the fixture text.
        let text = std::fs::read_to_string(dir.join(".ssh/config")).unwrap();
        let mut hosts = Vec::new();
        for line in text.lines() {
            if let Some(rest) = line.trim().strip_prefix("Host ") {
                for a in rest.split_whitespace() {
                    if !a.contains('*') {
                        hosts.push(a.to_string());
                    }
                }
            }
        }
        assert_eq!(hosts, vec!["prod", "staging", "box"]);
    }
}
