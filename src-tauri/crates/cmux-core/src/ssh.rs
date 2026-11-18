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

/// The command line an SSH pane's `ssh -tt` runs to start in `dir` there:
/// into it, then the person's login shell in its place. `;` rather than
/// `&&`: a directory gone since leaves a shell in the home, under cd's
/// own message, rather than a session that ends before it starts. Read
/// alike by sh, bash, zsh and fish, whichever is the login shell there.
pub fn shell_in(dir: &str) -> String {
    format!("cd {}; exec \"$SHELL\" -l", remote_dir(dir))
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
    let mut child = remote_sh(host).map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().expect("piped");
    let script = script.to_string();
    // From a thread: a large upload must not wait on ssh's output being read.
    let writer = std::thread::spawn(move || stdin.write_all(script.as_bytes()));
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    let _ = writer.join();
    remote_answer(host, &output)
}

/// The `ssh <host> sh -s` [`remote_shell`] runs, every stdio piped.
#[cfg(not(windows))]
fn remote_sh(host: &str) -> std::io::Result<std::process::Child> {
    let mut cmd = Command::new("ssh");
    // Never a password prompt: nobody is there to answer it.
    cmd.arg("-o").arg("BatchMode=yes");
    for opt in control_opts() {
        cmd.arg("-o").arg(opt);
    }
    cmd.args(host_args(host))
        .args(["sh", "-s"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
}

/// What a [`remote_sh`] that ended printed, or why it failed.
#[cfg(not(windows))]
fn remote_answer(host: &str, output: &std::process::Output) -> Result<String, String> {
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

/// Tests `host` as a computer for agents of this app's `version`, working
/// in `dir` there: one connection, which never prompts, and nothing made
/// or fetched there. An error is ssh's own — a host it cannot reach or
/// log in to — or an answer that is not the check's, or none within
/// [`CHECK_TIMEOUT`].
///
/// Run as [`remote_shell`] runs a script, over the host's shared master,
/// as an agent's start would, but ended when it takes too long. Ending it
/// ends the check's session alone. With `ControlPersist`, the ssh this
/// spawns is never the master a pane or an agent is on: one already
/// running, it joins as a client; none, it becomes one only until it has
/// logged in, then forks the master into the background, detached, and
/// carries on as that master's client.
#[cfg(not(windows))]
pub fn check_computer(host: &str, dir: &str, version: &str) -> Result<cmux_protocol::ComputerCheck, String> {
    let child = remote_sh(host).map_err(|e| e.to_string())?;
    let output = output_within(child, check_computer_script(dir, version), CHECK_TIMEOUT).map_err(|e| e.to_string())?;
    let Some(output) = output else {
        return Err(format!(
            "{host} did not answer the test within {}s: ssh may still be logging in, or a login file there (.profile, .bashrc, .zshrc…) runs something that does not end",
            CHECK_TIMEOUT.as_secs()
        ));
    };
    let answer = remote_answer(host, &output)?;
    computer_check_in(&answer, version).ok_or_else(|| format!("{host} did not answer the test: {}", answer.trim()))
}

/// How long a computer's test may take: twice the [`CONNECT_TIMEOUT`], so
/// a link that only just connects still has as long again to log in and
/// run it, and short enough for a person watching its button. Only the
/// test has it: a `mira` download may rightly take longer.
#[cfg_attr(windows, allow(dead_code))]
const CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// What `child` printed, given `input` on its stdin, if it ends within
/// `limit`; otherwise none, the child killed and reaped. Its stdio must
/// be piped.
#[cfg(not(windows))]
fn output_within(mut child: std::process::Child, input: String, limit: std::time::Duration) -> std::io::Result<Option<std::process::Output>> {
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};
    // Each pipe read to its end on a thread of its own, as
    // `wait_with_output` reads them, so a child with much to say is never
    // stuck on a full one.
    fn read_all(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    }
    let mut stdin = child.stdin.take().expect("piped");
    // Not joined: a child that never reads its input is killed at the
    // limit, and the write fails then.
    std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let stdout = read_all(child.stdout.take().expect("piped"));
    let stderr = read_all(child.stderr.take().expect("piped"));
    let deadline = Instant::now() + limit;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            // The readers are not joined: their pipes end with the child.
            let _ = child.kill();
            child.wait()?;
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    Ok(Some(std::process::Output { status, stdout, stderr }))
}

/// The script [`check_computer`] runs, printing `key=value` lines. Each
/// thing is looked at as an agent's start there would meet it. Its `mira`
/// as [`ENSURE_MIRA`] finds one. Its directory from the home, where its
/// holder starts, as [`remote_dir`] makes it the word the agent's `cd`
/// in [`run_script`] takes: the two never disagree. And `claude` under
/// the PATH of the login shell [`run_script`] goes through, given only
/// plain words, its stdin closed so it cannot read this script; anything
/// the person's rc files print comes before its last line.
#[cfg_attr(windows, allow(dead_code))]
fn check_computer_script(dir: &str, version: &str) -> String {
    [
        r#"printf 'os=%s\n' "$(uname -s)""#.to_string(),
        r#"printf 'arch=%s\n' "$(uname -m)""#.into(),
        format!(r#"if [ -x "$HOME/.mirador/bin/mira-{version}" ]; then echo mira=yes; else echo mira=no; fi"#),
        format!("if (cd {}) 2>/dev/null; then echo dir=yes; else echo dir=no; fi", remote_dir(dir)),
        r#"p=$("${SHELL:-/bin/sh}" -l -i -c 'printenv PATH' </dev/null 2>/dev/null | tail -n 1)"#.into(),
        r#"if [ -n "$p" ] && (PATH=$p; command -v claude) >/dev/null 2>&1; then echo claude=yes; else echo claude=no; fi"#.into(),
    ]
    .join("\n")
        + "\n"
}

/// What a [`check_computer_script`] answer says, or none when it is not
/// one: every key there, the last of each winning, since what a remote's
/// rc files print on connecting comes first, and none of them empty or
/// other than `yes` or `no` where that is the answer.
#[cfg_attr(windows, allow(dead_code))]
fn computer_check_in(answer: &str, version: &str) -> Option<cmux_protocol::ComputerCheck> {
    let get = |key: &str| {
        answer.lines().rev().find_map(|l| l.trim_end().strip_prefix(key)?.strip_prefix('=')).filter(|v| !v.is_empty())
    };
    let yes = |key: &str| match get(key)? {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    };
    let (os, arch) = (get("os")?, get("arch")?);
    let (mira, dir_found, claude_found) = (yes("mira")?, yes("dir")?, yes("claude")?);
    let system = format!("{os} {arch}");
    // ENSURE_MIRA's case for a published mira: change the two together.
    let published = os.eq_ignore_ascii_case("linux") && matches!(arch, "x86_64" | "aarch64" | "arm64");
    let agents = if !mira && !published {
        Some(format!("no mira is published for {system}: put a mira of version {version} at ~/.mirador/bin/mira-{version} there"))
    } else if !claude_found {
        Some("Claude Code is not on the PATH of a login shell there".to_string())
    } else if !dir_found {
        Some("the directory is not there".to_string())
    } else {
        None
    };
    Some(cmux_protocol::ComputerCheck { system, dir_found, claude_found, agents })
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
        format!(r#"cd {} || {{ echo "mirador: no directory {shown} on $(hostname)" >&2; exit 1; }}"#, remote_dir(dir)),
        // A forward that failed before this start shows here; the agent
        // still works, it just never reports.
        r#"[ -S "$MIRA_SOCKET" ] || echo "mirador: nothing listens at $MIRA_SOCKET on $(hostname): this agent will not report to Mirador" >&2"#.into(),
        format!("exec {launch}"),
    ]
    .join("\n")
        + "\n"
}

/// `dir` on a remote as the one word POSIX sh's `cd` takes there. A `~`
/// alone or before a `/` is the remote's home, as a terminal there reads
/// it: `"$HOME"`, expanded by that sh, with the rest single-quoted so
/// nothing else in it is read. Anything else is quoted whole. `~user` is
/// not expanded: it stays a directory named so, and is not found. One
/// that starts with `-` is given `./` first, so no word `cd` gets starts
/// with `-`: `-L` and `-P` are its options, and `-` is `$OLDPWD` to it
/// even after a `--`.
#[cfg_attr(windows, allow(dead_code))]
fn remote_dir(dir: &str) -> String {
    if dir == "~" {
        return r#""$HOME""#.into();
    }
    match dir.strip_prefix("~/") {
        Some(rest) => format!(r#""$HOME"/{}"#, quote(rest)),
        None if dir.starts_with('-') => quote(&format!("./{dir}")),
        None => quote(dir),
    }
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

    /// A computer's test, as read from its answer: its system, its
    /// directory, its `claude`, and whether agents can run there — and the
    /// one reason when they cannot.
    #[test]
    fn a_computers_test_says_whether_agents_can_run_there() {
        let answer = |os: &str, arch: &str, mira: &str, dir: &str, claude: &str| {
            format!("os={os}\narch={arch}\nmira={mira}\ndir={dir}\nclaude={claude}\n")
        };
        let check = computer_check_in(&answer("Linux", "x86_64", "no", "yes", "yes"), "1.2.3").unwrap();
        assert_eq!(
            check,
            cmux_protocol::ComputerCheck { system: "Linux x86_64".into(), dir_found: true, claude_found: true, agents: None }
        );
        for arch in ["aarch64", "arm64"] {
            assert_eq!(computer_check_in(&answer("Linux", arch, "no", "yes", "yes"), "1.2.3").unwrap().agents, None, "{arch}");
        }

        let missing = computer_check_in(&answer("Linux", "x86_64", "no", "no", "yes"), "1.2.3").unwrap();
        assert!(!missing.dir_found, "{missing:?}");
        assert_eq!(missing.agents.as_deref(), Some("the directory is not there"));

        let mac = computer_check_in(&answer("Darwin", "arm64", "no", "yes", "yes"), "1.2.3").unwrap();
        assert_eq!(mac.system, "Darwin arm64");
        assert_eq!(
            mac.agents.as_deref(),
            Some("no mira is published for Darwin arm64: put a mira of version 1.2.3 at ~/.mirador/bin/mira-1.2.3 there")
        );
        // One put there by hand is the one an agent uses, as ENSURE_MIRA does.
        assert_eq!(computer_check_in(&answer("Darwin", "arm64", "yes", "yes", "yes"), "1.2.3").unwrap().agents, None);
        let riscv = computer_check_in(&answer("Linux", "riscv64", "no", "yes", "yes"), "1.2.3").unwrap();
        assert!(riscv.agents.unwrap().starts_with("no mira is published for Linux riscv64"));

        let no_claude = computer_check_in(&answer("Linux", "x86_64", "no", "yes", "no"), "1.2.3").unwrap();
        assert!(!no_claude.claude_found);
        assert_eq!(no_claude.agents.as_deref(), Some("Claude Code is not on the PATH of a login shell there"));
    }

    /// What a remote's rc files print on connecting is not the answer, and
    /// an answer missing any part of the test, or saying something else
    /// than the test does, is none.
    #[test]
    fn a_computers_test_is_read_past_noise_and_never_guessed() {
        let noisy = "Welcome!\nos=Plan9\nlast login: today\nos=Linux\narch=x86_64\nmira=no\ndir=yes\nclaude=yes\r\n";
        assert_eq!(computer_check_in(noisy, "1").unwrap().system, "Linux x86_64");
        for odd in [
            "",
            "garbage\n",
            "sh: 1: Syntax error\n",
            "os=Linux\narch=x86_64\nmira=no\ndir=yes\n",
            "os=Linux\narch=x86_64\nmira=no\ndir=yes\nclaude=maybe\n",
            "os=\narch=x86_64\nmira=no\ndir=yes\nclaude=yes\n",
        ] {
            assert_eq!(computer_check_in(odd, "1"), None, "{odd:?}");
        }
    }

    /// The test's script, run by sh in a home of its own: `claude` found
    /// on the PATH only the login shell's profile sets, as an agent's run
    /// script finds it; the directory as the agent's `cd` sees it, from
    /// the home, a `~` the home too; and nothing made there.
    #[cfg(unix)]
    #[test]
    fn a_computers_test_sees_what_an_agent_would() {
        let home = std::env::temp_dir().join(format!("mira-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("work/app")).unwrap();
        std::fs::create_dir_all(home.join("tools")).unwrap();
        // The whole PATH: the system's profile would add its own, which may
        // hold a claude of this machine's.
        std::fs::write(home.join(".profile"), "echo welcome\nPATH=\"$HOME/tools:/usr/bin:/bin\"; export PATH\n").unwrap();
        let run = |dir: &str| {
            let output = Command::new("/bin/sh")
                .arg("-c")
                .arg(check_computer_script(dir, "1.2.3"))
                .env_clear()
                .env("HOME", &home)
                .env("SHELL", "/bin/sh")
                .env("PATH", "/usr/bin:/bin")
                .current_dir(&home)
                .output()
                .unwrap();
            computer_check_in(&String::from_utf8_lossy(&output.stdout), "1.2.3").unwrap()
        };
        let listing = || {
            let mut names: Vec<_> = std::fs::read_dir(&home).unwrap().map(|e| e.unwrap().file_name()).collect();
            names.sort();
            names
        };
        let before = listing();

        let check = run("work/app");
        assert!(check.dir_found && !check.claude_found, "{check:?}");
        let uname = |flag: &str| String::from_utf8_lossy(&Command::new("uname").arg(flag).output().unwrap().stdout).trim().to_string();
        assert_eq!(check.system, format!("{} {}", uname("-s"), uname("-m")));
        assert!(run(home.join("work/app").to_str().unwrap()).dir_found);
        assert!(run("~/work/app").dir_found, "a ~ is the home, for the agent's cd as here");
        assert!(run("~").dir_found);
        assert!(!run("~/nope").dir_found);
        assert!(!run("work/none").dir_found);

        let claude = home.join("tools/claude");
        std::fs::write(&claude, "#!/bin/sh\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(run("work/app").claude_found, "on the login shell's PATH");

        assert_eq!(listing(), before, "nothing made in the home");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A child that answers in time gives what it printed on both pipes,
    /// its input read and its exit code kept, as `remote_answer` reads them.
    #[cfg(not(windows))]
    #[test]
    fn a_bounded_run_that_answers_in_time_gives_its_output() {
        let child = piped(Command::new("/bin/sh").arg("-s"));
        let output = output_within(child, "echo out; echo err >&2; exit 3\n".into(), std::time::Duration::from_secs(10))
            .unwrap()
            .expect("in time");
        assert_eq!((output.stdout.as_slice(), output.stderr.as_slice(), output.status.code()), (&b"out\n"[..], &b"err\n"[..], Some(3)));
    }

    /// A child that outlives the bound is given up on at the bound, and
    /// ended: nothing of it is left running.
    #[cfg(not(windows))]
    #[test]
    fn a_bounded_run_that_takes_too_long_is_ended() {
        let child = piped(Command::new("sleep").arg("30"));
        let pid = child.id().to_string();
        let start = std::time::Instant::now();
        let output = output_within(child, String::new(), std::time::Duration::from_millis(300)).unwrap();
        assert!(output.is_none());
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "{:?}", start.elapsed());
        let alive = Command::new("kill").args(["-0", &pid]).stderr(std::process::Stdio::null()).status().unwrap().success();
        assert!(!alive, "sleep {pid} still runs");
    }

    #[cfg(not(windows))]
    fn piped(cmd: &mut Command) -> std::process::Child {
        use std::process::Stdio;
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()
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

    /// A remote directory as `cd`'s word: a leading `~` the home, expanded
    /// there, the rest quoted whole, `~user` left as typed. What the sh
    /// that runs these words makes of them is pinned by the next test.
    #[test]
    fn a_remote_dir_is_one_word_with_its_tilde_the_home() {
        assert_eq!(remote_dir("~"), r#""$HOME""#);
        assert_eq!(remote_dir("~/a b"), r#""$HOME"/'a b'"#);
        assert_eq!(remote_dir("/abs"), "'/abs'");
        assert_eq!(remote_dir("rel"), "'rel'");
        assert_eq!(remote_dir("~user/x"), "'~user/x'");
        assert_eq!(remote_dir("~/it's $HOME"), r#""$HOME"/'it'\''s $HOME'"#);
        assert_eq!(remote_dir("/a/$(x)"), "'/a/$(x)'");
        assert_eq!(remote_dir("-"), "'./-'");
        assert_eq!(remote_dir("-L"), "'./-L'");
        assert_eq!(remote_dir("-x"), "'./-x'");
    }

    /// The `cd` of an agent's run script, run by a real sh from the home,
    /// where a remote's holder starts it: a `~/` dir lands in the home's,
    /// nothing else in it read by the shell; `-` and `-L` are directories
    /// there, never `$OLDPWD`, an option or the home; and a missing one
    /// says what was typed.
    #[cfg(unix)]
    #[test]
    fn a_remote_agents_cd_takes_its_tilde_as_the_home() {
        let home = std::env::temp_dir().join(format!("mira-cd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("work/a $b")).unwrap();
        let home = home.canonicalize().unwrap();
        let run = |dir: &str| {
            let script = run_script("/unused", "p1", "/nowhere.sock", dir, "pwd");
            // Past the login-shell hop, which this test is not about.
            Command::new("/bin/sh")
                .arg("-c")
                .arg(script)
                .env_clear()
                .env("HOME", &home)
                .env("OLDPWD", home.join("work"))
                .env("MIRA_RUN", "agent")
                .env("PATH", "/usr/bin:/bin")
                .current_dir(&home)
                .output()
                .unwrap()
        };
        let landed = |dir: &str| String::from_utf8_lossy(&run(dir).stdout).trim().to_string();
        assert_eq!(landed("~/work"), home.join("work").to_str().unwrap());
        assert_eq!(landed("~"), home.to_str().unwrap());
        assert_eq!(landed("~/work/a $b"), home.join("work/a $b").to_str().unwrap());
        for dash in ["-", "-L"] {
            let missing = run(dash);
            assert_eq!((missing.status.code(), missing.stdout.as_slice()), (Some(1), &b""[..]), "{dash}: {missing:?}");
            std::fs::create_dir_all(home.join(dash)).unwrap();
            assert_eq!(landed(dash), home.join(dash).to_str().unwrap());
        }
        let missing = run("~/nope");
        assert_eq!(missing.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&missing.stderr).contains("mirador: no directory ~/nope on "), "{missing:?}");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// An SSH pane's start in its directory, run by a real sh as the
    /// remote's login shell runs what ssh hands it, from the home: in
    /// `~/work` the shell it ends in is there. With the directory gone it
    /// is still a shell, in the home, under cd's own message, rather than
    /// a session that ends.
    #[cfg(unix)]
    #[test]
    fn an_ssh_pane_starts_in_its_directory_or_else_the_home() {
        use std::io::Write;
        let home = std::env::temp_dir().join(format!("mira-shell-in-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("work")).unwrap();
        let home = home.canonicalize().unwrap();
        let run = |dir: &str| {
            let mut sh = Command::new("/bin/sh")
                .arg("-c")
                .arg(shell_in(dir))
                .env_clear()
                .env("HOME", &home)
                .env("SHELL", "/bin/sh")
                .env("PATH", "/usr/bin:/bin")
                .current_dir(&home)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            // What the person types into the shell it gives.
            sh.stdin.take().unwrap().write_all(b"pwd\n").unwrap();
            sh.wait_with_output().unwrap()
        };
        let there = run("~/work");
        assert!(there.status.success(), "{there:?}");
        assert_eq!(String::from_utf8_lossy(&there.stdout).trim(), home.join("work").to_str().unwrap());
        let gone = run("~/gone");
        assert!(gone.status.success(), "a shell all the same: {gone:?}");
        assert_eq!(String::from_utf8_lossy(&gone.stdout).trim(), home.to_str().unwrap());
        assert!(String::from_utf8_lossy(&gone.stderr).contains("gone"), "cd says why: {gone:?}");
        let _ = std::fs::remove_dir_all(&home);
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
