//! SSH remote workspaces. A remote pane runs `ssh -tt <host>` in a local
//! PTY, so the system ssh handles auth, agent, 2FA, and ProxyJump exactly
//! as it would in any terminal. On macOS/Linux a shared ControlMaster
//! connection lets us run port-forward control commands without
//! re-authenticating; Windows' OpenSSH has no ControlMaster, so a forward
//! there is its own `ssh -N -L` process (one extra authentication).
//!
//! `host` is passed through as ssh arguments split on whitespace, so a bare
//! alias (`prod`, resolved via ~/.ssh/config) is the common case, while a
//! full destination (`-p 2222 user@host`) also works.

use std::path::PathBuf;
#[cfg(not(windows))]
use std::process::Command;

use portable_pty::CommandBuilder;

/// In the runtime dir, beside the holders: a control socket is runtime
/// state, and its path has to fit the 104-byte socket limit, which a long
/// config directory once broke ("ControlPath too long", every SSH pane).
#[cfg(not(windows))]
fn control_dir() -> PathBuf {
    control_dir_in(&crate::config::runtime_dir())
}

#[cfg(not(windows))]
fn control_dir_in(runtime_dir: &std::path::Path) -> PathBuf {
    runtime_dir.join("ssh")
}

/// `%C` is a short stable hash of (host, port, user, proxy) — safe for a
/// socket path and shared between the interactive session and control ops.
#[cfg(not(windows))]
fn control_path() -> String {
    control_dir().join("cm-%C").to_string_lossy().into_owned()
}

/// The options that put an ssh on the shared master, its directory made:
/// ssh does not make it, and refuses to bind a master in one that is not
/// there — which an agent on a remote, reaching it before any SSH pane
/// has opened, met.
#[cfg(not(windows))]
fn control_opts() -> Vec<String> {
    let _ = std::fs::create_dir_all(control_dir());
    vec![
        "ControlMaster=auto".to_string(),
        format!("ControlPath={}", control_path()),
        "ControlPersist=30m".to_string(),
        CONNECT_TIMEOUT.to_string(),
    ]
}

/// Windows' OpenSSH rejects the multiplexing options outright.
#[cfg(windows)]
fn control_opts() -> Vec<String> {
    vec![CONNECT_TIMEOUT.to_string()]
}

/// A host that is down is given up on in seconds, not after the system's
/// TCP timeout: a remote agent's pane retries its connection, and each
/// retry makes sure of the app's forward first.
const CONNECT_TIMEOUT: &str = "ConnectTimeout=15";

/// Splits a host spec into ssh args (`"-p 2222 host"` → `["-p","2222","host"]`).
fn host_args(host: &str) -> Vec<String> {
    host.split_whitespace().map(String::from).collect()
}

/// The interactive `ssh -tt` command for a remote pane: a login shell, or
/// with `command`, that command line there, as one argument: ssh hands it
/// to the remote's login shell, which parses it. For an agent it is
/// `'<mira>' attach <pane>`, which any shell, fish included, reads alike.
pub fn interactive_command(host: &str, command: Option<&str>) -> CommandBuilder {
    let mut cmd = CommandBuilder::new("ssh");
    cmd.arg("-tt");
    for opt in control_opts() {
        cmd.arg("-o");
        cmd.arg(opt);
    }
    for arg in host_args(host) {
        cmd.arg(arg);
    }
    if let Some(command) = command {
        cmd.arg(command);
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd
}

/// Establishes (or cancels) a local port forward over the pane's existing
/// ControlMaster: `localhost:<port>` → the remote's `localhost:<port>`.
/// Requires the interactive session (the master) to be connected.
#[cfg(not(windows))]
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

/// Makes this app's socket, `local`, reachable on `host` over the pane's
/// ControlMaster, as [`forward`] does a port, so a `mira` run there
/// reaches this app through [`crate::ipc::SOCKET_ENV`]. Returns the
/// socket's path on the remote. sshd creates it owner-only (its
/// `StreamLocalBindMask` is 0177 by default): nobody else on that
/// computer reaches the app through it.
#[cfg(not(windows))]
pub fn forward_socket(host: &str, local: &std::path::Path) -> Result<String, String> {
    // Every pane of a host shares the one path, and the steps below are
    // three: two forwards of it interleaved — one's rm between the other's
    // rm and forward, then a forward that is a no-op over the same master —
    // would leave nothing serving it, and no error anywhere. One at a time,
    // whichever thread asks (a pane starting, `agent new --host`, a team).
    //
    // And a forward that works is left alone. Redoing it takes the socket
    // away for a moment, and a teammate's hook connecting then is lost
    // without a word: a team's members each forwarded again as their panes
    // started, and one's first status never came. A forward lasts as long
    // as the master it was made on, so each is kept with that master's
    // pid, and done again only once another master holds the connection.
    static FORWARDED: std::sync::Mutex<Vec<Forwarded>> = std::sync::Mutex::new(Vec::new());
    let mut forwarded = FORWARDED.lock().unwrap_or_else(|e| e.into_inner());
    let master = master_pid(host);
    if let Some(f) = forwarded.iter().find(|f| f.host == host && f.local == local && Some(f.master) == master) {
        return Ok(f.remote.clone());
    }
    // Being redone, it has no record until it is made: a redo that fails
    // half-way must not leave one saying it works.
    forwarded.retain(|f| !(f.host == host && f.local == local));
    let dir = remote_shell(host, r#"printf %s "${XDG_RUNTIME_DIR:-/tmp}""#)?;
    let remote = remote_socket_path(&dir, local);
    let spec = format!("{remote}:{}", local.display());
    let control = |action: &str| {
        let mut cmd = Command::new("ssh");
        cmd.arg("-O").arg(action);
        for opt in control_opts() {
            cmd.arg("-o").arg(opt);
        }
        cmd.arg("-R").arg(&spec).args(host_args(host));
        cmd.output().map_err(|e| e.to_string())
    };
    // Forwarded again over the same master, the same forward is a no-op:
    // after the rm below, nothing would serve the path. So any forward of
    // it is let go first; with none, ssh says so and nothing changes.
    let _ = control("cancel");
    // sshd does not bind over a file already there (`StreamLocalBindUnlink`
    // is off by default), and an earlier forward, a cancelled one or one
    // from before a restart, leaves its socket behind.
    remote_shell(host, &format!("rm -f '{}'", remote.replace('\'', r"'\''")))?;
    let output = control("forward")?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("could not forward the app's socket to {host}: {}", err.trim()));
    }
    if let Some(master) = master_pid(host) {
        forwarded.push(Forwarded { host: host.to_string(), local: local.to_path_buf(), master, remote: remote.clone() });
    }
    Ok(remote)
}

/// A forward of this app's socket that works: made on `host`'s master
/// `master` (its pid), from `local`, to `remote` there.
#[cfg(not(windows))]
struct Forwarded {
    host: String,
    local: std::path::PathBuf,
    master: u32,
    remote: String,
}

/// The pid of the ControlMaster holding `host`'s connection, if one runs.
#[cfg(not(windows))]
fn master_pid(host: &str) -> Option<u32> {
    let mut cmd = Command::new("ssh");
    cmd.arg("-O").arg("check");
    for opt in control_opts() {
        cmd.arg("-o").arg(opt);
    }
    let output = cmd.args(host_args(host)).output().ok()?;
    output.status.success().then(|| master_pid_in(&String::from_utf8_lossy(&output.stderr))).flatten()
}

/// The pid in `ssh -O check`'s answer, `Master running (pid=26180)`.
#[cfg_attr(windows, allow(dead_code))]
fn master_pid_in(answer: &str) -> Option<u32> {
    answer.split_once("(pid=")?.1.split_once(')')?.0.parse().ok()
}

/// Runs `script` on `host` over the pane's connection, and gives what it
/// printed. The script goes to `sh -s` on stdin, never on the command
/// line: nothing in it is quoted for the remote's login shell, which may
/// be fish, whose quoting is not POSIX. A command in it that reads stdin
/// would read the rest of the script, so scripts redirect any that does.
#[cfg(not(windows))]
fn remote_shell(host: &str, script: &str) -> Result<String, String> {
    use std::io::Write;
    let mut cmd = Command::new("ssh");
    // Never a password prompt: nobody is there to answer it.
    cmd.arg("-o").arg("BatchMode=yes");
    for opt in control_opts() {
        cmd.arg("-o").arg(opt);
    }
    let mut child = cmd
        .args(host_args(host))
        .args(["sh", "-s"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().expect("piped");
    let script = script.to_string();
    // From a thread: a large upload must not wait on ssh's output being read.
    let writer = std::thread::spawn(move || stdin.write_all(script.as_bytes()));
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    let _ = writer.join();
    let err = String::from_utf8_lossy(&output.stderr);
    match output.status.code() {
        Some(0) => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        // ssh's own failure — a host it cannot reach or log in to; it runs
        // on a connection of its own, before any pane may exist. Anything
        // else is the command's.
        Some(255) | None => Err(format!("ssh {host}: {}", err.trim())),
        Some(_) => Err(format!("on {host}: {}", err.trim())),
    }
}

/// What a remote runs to find the `mira` of this app's version, or fetch
/// it from that version's release: `~/.mirador/bin/mira-<version>`. Only
/// Linux on x86_64 or aarch64 is published; anywhere else it exits 2,
/// saying where a person may put one by hand. Release assets lose their
/// executable bit, hence the chmod. POSIX sh, whatever the login shell.
#[cfg_attr(windows, allow(dead_code))]
const ENSURE_MIRA: &str = r#"d="$HOME/.mirador/bin"
f="$d/mira-@VERSION@"
if [ -x "$f" ]; then printf %s "$f"; exit 0; fi
os=$(uname -s | tr A-Z a-z)
m=$(uname -m)
case "$m" in x86_64) arch=x86_64 ;; aarch64|arm64) arch=aarch64 ;; *) arch= ;; esac
if [ "$os" != linux ] || [ -z "$arch" ]; then
  echo "no mira is published for $os/$m: put a mira of version @VERSION@ at $f" >&2
  exit 2
fi
url="https://github.com/@REPO@/releases/download/v@VERSION@/mira-linux-$arch"
mkdir -p "$d" || exit 1
if command -v curl >/dev/null 2>&1; then
  curl -fsSL -o "$f.tmp" "$url"
elif command -v wget >/dev/null 2>&1; then
  wget -qO "$f.tmp" "$url"
else
  echo "neither curl nor wget is there to fetch $url" >&2
  false
fi || { rm -f "$f.tmp"; echo "could not fetch $url" >&2; exit 1; }
chmod +x "$f.tmp" && mv "$f.tmp" "$f" && printf %s "$f"
"#;

#[cfg_attr(windows, allow(dead_code))]
fn ensure_mira_script(version: &str) -> String {
    ENSURE_MIRA.replace("@VERSION@", version).replace("@REPO@", crate::REPO)
}

/// The path of a `mira` of `version` on `host`, fetched from that
/// version's release the first time ([`ENSURE_MIRA`]): nothing is copied
/// from this machine, whose `mira` is built for another system.
#[cfg(not(windows))]
pub fn ensure_mira(host: &str, version: &str) -> Result<String, String> {
    let path = remote_shell(host, &ensure_mira_script(version))?;
    Ok(path.trim().to_string())
}

/// The remote's home directory, where an agent's run script goes. It ends
/// up in commands given to ssh as plain words, so one with a space or
/// anything a shell would read is refused rather than quoted for a shell
/// Mirador cannot know.
#[cfg(not(windows))]
pub fn remote_home(host: &str) -> Result<String, String> {
    let home = remote_shell(host, r#"printf %s "$HOME""#)?;
    if !home.starts_with('/') || !is_plain_path(&home) {
        return Err(format!("{host}'s home directory, `{home}`, is not one an agent can be started in from Mirador yet"));
    }
    Ok(home)
}

/// Whether `path` is one every shell reads as one word, unquoted.
pub fn is_plain_path(path: &str) -> bool {
    !path.is_empty() && path.chars().all(|c| c.is_ascii_alphanumeric() || "/._-".contains(c))
}

/// The script an agent runs on a remote, saved there as `run` (a plain
/// path). The agent's holder there runs it (`mira __hold-start`), so the
/// agent outlives the connection and the pane only attaches to it. It
/// runs twice. First through the person's login shell, as
/// an interactive login shell: what a terminal of theirs there would be,
/// which reads their profile and their rc file alike. A command over ssh
/// otherwise gets neither, and `claude` installed through nvm or `npm -g`
/// is only on the PATH those set (nvm's installer writes to `.bashrc` or
/// `.zshrc`, which Ubuntu's own `.bashrc` skips unless interactive). That
/// shell is given nothing but `exec /bin/sh <run>`, plain words, so its
/// own syntax never matters, fish included. Then `sh` again, under that
/// PATH: it goes to `dir`, saying so loudly if there is none, and becomes
/// `launch`, quoted for POSIX sh, with the pane and the app's socket in
/// its environment.
#[cfg_attr(windows, allow(dead_code))]
pub fn run_script(run: &str, pane: &str, socket: &str, dir: &str, launch: &str) -> String {
    let shown = dir.replace(['"', '\\', '$', '`'], "_");
    [
        r#"if [ "$MIRA_RUN" != agent ]; then"#.to_string(),
        format!("  export MIRA_RUN=agent MIRA_PANE={} MIRA_SOCKET={}", quote(pane), quote(socket)),
        format!(r#"  exec "${{SHELL:-/bin/sh}}" -l -i -c 'exec /bin/sh {run}'"#),
        "fi".into(),
        "unset MIRA_RUN".into(),
        format!(r#"cd {} || {{ echo "mirador: no directory {shown} on $(hostname)" >&2; exit 1; }}"#, quote(dir)),
        // A forward that failed before this start shows here; the agent
        // still works, it just never reports.
        r#"[ -S "$MIRA_SOCKET" ] || echo "mirador: nothing listens at $MIRA_SOCKET on $(hostname): this agent will not report to Mirador" >&2"#.into(),
        format!("exec {launch}"),
    ]
    .join("\n")
        + "\n"
}

/// `s` as one word for POSIX sh.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Starts the holder an agent on `host` runs under, running its run script
/// (`run`, from [`run_script`]) as `pane`, with the remote's `mira`; returns
/// once it answers. The agent then outlives any connection, and its pane
/// attaches to it with [`attach_line`].
#[cfg(not(windows))]
pub fn start_held(host: &str, mira: &str, pane: &str, run: &str) -> Result<(), String> {
    remote_shell(host, &start_held_script(mira, pane, run)).map(|_| ())
}

/// The script [`start_held`] runs. TERM is set here: a command over ssh
/// with no tty has none to pass on, and the holder's terminal keeps the
/// one it starts with. No `--cwd`: the run script goes to its directory
/// itself, and says so when there is none.
#[cfg_attr(windows, allow(dead_code))]
fn start_held_script(mira: &str, pane: &str, run: &str) -> String {
    format!(
        "TERM=xterm-256color COLORTERM=truecolor {} __hold-start --pane {} --command {}\n",
        quote(mira),
        quote(pane),
        quote(&format!("sh {run}"))
    )
}

/// The command line a remote agent's pane runs over `ssh -tt`: attaching
/// to its holder there. Both are plain words (`remote_home` saw to the
/// home), so quoted they read the same to fish as to sh.
pub fn attach_line(mira: &str, pane: &str) -> String {
    format!("{} attach {}", quote(mira), quote(pane))
}

/// The remote's `mira` in a line from [`attach_line`].
pub fn mira_of(attach_line: &str) -> Option<String> {
    let (mira, _) = attach_line.strip_prefix('\'')?.split_once("' attach ")?;
    Some(mira.replace(r"'\''", "'"))
}

/// Ends `pane`'s agent on `host`: its holder there, through the remote's
/// `mira`, which waits for it to go.
#[cfg(not(windows))]
pub fn end_held(host: &str, mira: &str, pane: &str) -> Result<(), String> {
    remote_shell(host, &format!("{} __hold-end --pane {}\n", quote(mira), quote(pane))).map(|_| ())
}

/// Writes `bytes` to `path` on `host`, exactly, owner-only (0600: prompts
/// and settings are the person's), making its directory if needed.
#[cfg(not(windows))]
pub fn put_file(host: &str, path: &str, bytes: &[u8]) -> Result<(), String> {
    remote_shell(host, &put_file_script(path, bytes)).map(|_| ())
}

/// The script [`put_file`] runs. The bytes travel as base64 in a quoted
/// heredoc rather than as themselves: a heredoc always ends its text with
/// a newline, so a file without one could not arrive exact, and any text
/// may hold a line equal to the delimiter. Base64 holds no `_`, so
/// `MIRA_EOF` never occurs in it, and `base64 -d` is in coreutils, busybox
/// and macOS alike. Written beside the file then moved, so it appears whole
/// and 0600 from the start (umask 077).
#[cfg_attr(windows, allow(dead_code))]
fn put_file_script(path: &str, bytes: &[u8]) -> String {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    let lines: Vec<&str> = encoded.as_bytes().chunks(76).map(|c| std::str::from_utf8(c).unwrap()).collect();
    let dir = path.rsplit_once('/').map(|(d, _)| d).filter(|d| !d.is_empty());
    let tmp = format!("{path}.tmp");
    format!(
        "set -e\numask 077\n{mkdir}base64 -d > {tmp} <<'MIRA_EOF'\n{body}\nMIRA_EOF\nchmod 600 {tmp}\nmv {tmp} {path}\n",
        mkdir = dir.map(|d| format!("mkdir -p {}\n", quote(d))).unwrap_or_default(),
        tmp = quote(&tmp),
        body = lines.join("\n"),
        path = quote(path),
    )
}

/// Where the app's socket `local` goes on a remote whose runtime directory
/// is `dir`: named for `local`, which only one running app serves and the
/// same app serves again after a restart, so agents told the path once
/// still reach it after an update. Short, because a socket path over ~104
/// bytes cannot be bound there either; `/tmp` when `dir` will not do.
pub fn remote_socket_path(dir: &str, local: &std::path::Path) -> String {
    // FNV-1a: a few lines, and the same in every Rust release, which
    // std's hasher does not promise.
    let hash = local.to_string_lossy().bytes().fold(0x811c_9dc5_u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193));
    let name = format!("mira-{hash:08x}.sock");
    let dir = dir.trim().trim_end_matches('/');
    if dir.starts_with('/') && dir.len() + 1 + name.len() < 104 {
        format!("{dir}/{name}")
    } else {
        format!("/tmp/{name}")
    }
}

/// Windows: no connection to piggyback on, so each forward is a dedicated
/// `ssh -N -L` child that Mirador owns and kills on cancel (or on exit,
/// since it dies with its parent's process tree). Authentication happens
/// again here — agent/key setups are unaffected, password logins prompt.
#[cfg(windows)]
pub fn forward(host: &str, port: u16, cancel: bool) -> Result<(), String> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    static FORWARDS: OnceLock<Mutex<HashMap<(String, u16), std::process::Child>>> = OnceLock::new();
    let forwards = FORWARDS.get_or_init(Default::default);

    let key = (host.to_string(), port);
    if cancel {
        let mut map = forwards.lock().unwrap();
        let mut child = map
            .remove(&key)
            .ok_or_else(|| format!("port {port} is not forwarded"))?;
        let _ = child.kill();
        let _ = child.wait();
        return Ok(());
    }
    if forwards.lock().unwrap().contains_key(&key) {
        return Err(format!("port {port} is already forwarded"));
    }

    let mut cmd = crate::proc::command("ssh");
    cmd.arg("-N")
        .arg("-o")
        .arg("ExitOnForwardFailure=yes")
        .arg("-L")
        .arg(format!("{port}:localhost:{port}"))
        .args(host_args(host))
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;

    // A forward that is going to fail (port in use, auth refused) fails
    // immediately — surface that instead of reporting a dead forward as up.
    std::thread::sleep(std::time::Duration::from_millis(400));
    if let Ok(Some(status)) = child.try_wait() {
        let mut err = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            use std::io::Read;
            let _ = stderr.read_to_string(&mut err);
        }
        let err = err.trim();
        return Err(if err.is_empty() {
            format!("port forward failed ({status})")
        } else {
            format!("port forward failed: {err}")
        });
    }
    forwards.lock().unwrap().insert(key, child);
    Ok(())
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
    let Some(home) = crate::config::home_dir() else {
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

    /// The app's socket on a remote: in its runtime directory, named for
    /// the local socket — the same name after a restart, another for
    /// another instance — or in /tmp when that directory is missing, not
    /// absolute, or too deep for a socket path.
    #[test]
    fn the_apps_socket_on_a_remote_is_short_and_this_instances() {
        let local = std::path::Path::new("/tmp/mira.sock");
        // FNV-1a of the path, pinned: a change here strands remote agents.
        assert_eq!(remote_socket_path("/run/user/1000", local), "/run/user/1000/mira-5ee79133.sock");
        assert_eq!(remote_socket_path("/run/user/1000/\n", local), "/run/user/1000/mira-5ee79133.sock");
        assert_eq!(
            remote_socket_path("/tmp", std::path::Path::new("/run/user/1000/mira.sock")),
            "/tmp/mira-2e8a4af1.sock"
        );
        for dir in ["", "/", "run/user", &format!("/{}", "d".repeat(100))] {
            assert_eq!(remote_socket_path(dir, local), "/tmp/mira-5ee79133.sock", "{dir}");
        }
    }

    /// A master's pid, from `ssh -O check`'s answer; none from anything else.
    #[test]
    fn a_masters_pid_is_read_from_its_check() {
        assert_eq!(master_pid_in("Master running (pid=26180)\n"), Some(26180));
        assert_eq!(master_pid_in("Control socket connect(/tmp/cm-x): No such file or directory\n"), None);
        assert_eq!(master_pid_in(""), None);
    }

    /// The script a remote runs for its `mira`: this version's file under
    /// ~/.mirador/bin, fetched from this version's release for its arch.
    #[test]
    fn a_remote_fetches_the_mira_of_this_version() {
        let script = ensure_mira_script("1.2.3");
        assert!(script.contains("f=\"$d/mira-1.2.3\""), "{script}");
        assert!(script.starts_with("d=\"$HOME/.mirador/bin\"\n"), "{script}");
        assert!(
            script.contains("url=\"https://github.com/SuperHomer/mirador/releases/download/v1.2.3/mira-linux-$arch\""),
            "{script}"
        );
        assert!(!script.contains('@'), "every placeholder filled: {script}");
    }

    /// An agent's run script: through the login shell with nothing but
    /// plain words, then in its directory with the pane and the socket in
    /// its environment, as the launch line — everything quoted for sh.
    #[test]
    fn a_remote_agent_runs_through_the_login_shell_then_sh() {
        let run = "/home/me/.mirador/run/p1.sh";
        let script = run_script(run, "p1", "/run/user/1/mira-5ee79133.sock", "/srv/it's here", "claude 'look around'");
        assert_eq!(
            script,
            [
                r#"if [ "$MIRA_RUN" != agent ]; then"#,
                "  export MIRA_RUN=agent MIRA_PANE='p1' MIRA_SOCKET='/run/user/1/mira-5ee79133.sock'",
                r#"  exec "${SHELL:-/bin/sh}" -l -i -c 'exec /bin/sh /home/me/.mirador/run/p1.sh'"#,
                "fi",
                "unset MIRA_RUN",
                r#"cd '/srv/it'\''s here' || { echo "mirador: no directory /srv/it's here on $(hostname)" >&2; exit 1; }"#,
                r#"[ -S "$MIRA_SOCKET" ] || echo "mirador: nothing listens at $MIRA_SOCKET on $(hostname): this agent will not report to Mirador" >&2"#,
                "exec claude 'look around'",
                "",
            ]
            .join("\n")
        );
        let odd = run_script(run, "p1", "/s", "/a\"$(x)`y`\\b", "claude");
        assert!(odd.contains(r#"no directory /a__(x)_y__b on"#), "{odd}");
    }

    /// A path every shell reads as one word, and those it might not.
    #[test]
    fn plain_paths_are_one_word_in_any_shell() {
        assert!(is_plain_path("/home/me-2/.mirador/run/0f3a_b.sh"));
        for odd in ["", "/home/my name", "/home/$USER", "/home/it's", "/home/a;b", "~/x", "/home/*"] {
            assert!(!is_plain_path(odd), "{odd}");
        }
    }

    /// A remote agent pane runs its command line there as one argument
    /// after the host, for the remote's shell to parse.
    #[test]
    fn a_remote_agent_pane_runs_its_command_line() {
        let dbg = format!("{:?}", interactive_command("box", Some("'/home/me/.mirador/bin/mira-1' attach p1")));
        assert!(dbg.contains(r#""box", "'/home/me/.mirador/bin/mira-1' attach p1""#), "{dbg}");
    }

    /// A file sent to a remote: base64 between delimiters no base64 holds,
    /// decoding to the exact bytes — a quote, `$HOME`, a backslash, a line
    /// equal to the delimiter, no final newline — written aside, made 0600,
    /// then moved into place, its directory made first.
    #[test]
    fn a_file_sent_to_a_remote_arrives_exact() {
        use base64::Engine;
        let text = "it's $HOME \\ here\nMIRA_EOF\nEOF\nno newline at the end";
        let script = put_file_script("/h/it's/probe.txt", text.as_bytes());
        let body = script.split_once("<<'MIRA_EOF'\n").unwrap().1.split_once("\nMIRA_EOF\n").unwrap().0;
        assert!(!body.contains("MIRA_EOF") && body.lines().all(|l| l.len() <= 76), "{script}");
        let decoded = base64::engine::general_purpose::STANDARD.decode(body.replace('\n', "")).unwrap();
        assert_eq!(decoded, text.as_bytes());
        assert!(script.starts_with("set -e\numask 077\nmkdir -p '/h/it'\\''s'\n"), "{script}");
        assert!(
            script.ends_with("chmod 600 '/h/it'\\''s/probe.txt.tmp'\nmv '/h/it'\\''s/probe.txt.tmp' '/h/it'\\''s/probe.txt'\n"),
            "{script}"
        );
        assert!(!put_file_script("probe.txt", b"x").contains("mkdir"), "no directory to make");
    }

    /// The holder starts the run script, quoted for sh, with a terminal
    /// type; the pane attaches with the same `mira`.
    #[test]
    fn a_remote_agent_starts_held_and_its_pane_attaches() {
        assert_eq!(
            start_held_script("/home/it's/.mirador/bin/mira-1", "p1", "/home/me/.mirador/run/p1.sh"),
            "TERM=xterm-256color COLORTERM=truecolor '/home/it'\\''s/.mirador/bin/mira-1' __hold-start --pane 'p1' --command 'sh /home/me/.mirador/run/p1.sh'\n"
        );
        assert_eq!(attach_line("/home/me/.mirador/bin/mira-1", "p1"), "'/home/me/.mirador/bin/mira-1' attach 'p1'");
    }

    /// The `mira` a remote pane attaches with is the one that ends it.
    #[test]
    fn the_mira_comes_back_out_of_an_attach_line() {
        for mira in ["/home/me/.mirador/bin/mira-1", "/home/it's/mira"] {
            assert_eq!(mira_of(&attach_line(mira, "p1")).as_deref(), Some(mira));
        }
        assert_eq!(mira_of("sh /home/me/run.sh"), None);
    }

    #[test]
    fn interactive_command_uses_ssh_tt_and_control_master() {
        let cmd = interactive_command("myhost", None);
        let dbg = format!("{cmd:?}");
        assert!(dbg.contains("ssh"));
        assert!(dbg.contains("-tt"));
        // Multiplexing is a unix-only capability of OpenSSH.
        assert_eq!(dbg.contains("ControlMaster=auto"), !cfg!(windows));
        assert!(dbg.contains("myhost"));
    }

    /// What OpenSSH binds for a ControlPath: `%C` expanded to 40 hex
    /// digits, and the `.` and 16 random characters of the name it listens
    /// on before renaming. It refuses one of 104 bytes or more.
    #[cfg(not(windows))]
    fn bound_len(path: &str) -> usize {
        path.replace("%C", &"f".repeat(40)).len() + 17
    }

    /// The control socket is not in the config directory, whose length
    /// is the user's (a sandbox's under $TMPDIR broke every SSH pane), and
    /// fits in the runtime dir of an e2e sandbox and of a long home. The
    /// budget: a runtime dir of up to 38 bytes, so a home of up to 29.
    #[cfg(not(windows))]
    #[test]
    fn the_control_socket_is_runtime_state_and_fits() {
        let path = control_path();
        assert!(path.starts_with(&*crate::config::runtime_dir().to_string_lossy()), "{path}");
        assert!(!path.starts_with(&*crate::config::config_dir().to_string_lossy()), "{path}");
        for runtime in ["/tmp/me2e99999crosstab/mirador", "/Users/firstname.lastname/.mirador"] {
            let path = control_dir_in(std::path::Path::new(runtime)).join("cm-%C").to_string_lossy().into_owned();
            assert!(bound_len(&path) < 104, "{path}: {}", bound_len(&path));
        }
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
