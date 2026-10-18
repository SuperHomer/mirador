//! End-to-end tests: the real Mirador, started in a throwaway sandbox and
//! driven over its socket, the way a person checks a change by hand (see
//! CLAUDE.md, *Verifying a change*) — written down, so it runs again.
//!
//! A [`Sandbox`] is a home directory, config directory and runtime
//! directory of its own: its socket, session, scrollback and prompts never
//! touch the real app's, and its shell reads an empty `.zshrc`. Agents in
//! it are `fake-agent` linked in as `claude` and `codex`, so a test needs
//! no login and spends nothing, and can make an agent do what it needs.
//! [`Sandbox::live`] is the exception: the release bundle with the real
//! agents, behind `MIRA_E2E_LIVE=1` ([`live!`]), never in CI.
//!
//! What it cannot see is the window: the app draws in a webview no
//! automation reaches. It sees what the app reports — the workspace, a
//! pane's screen, the agents — which is what most breakage changes.
//!
//! Needs, before `MIRA_E2E=1 cargo test -p cmux-e2e`:
//!
//! ```text
//! npm run build                      # the frontend a debug build loads
//! npm run build:cli                  # a fresh mira sidecar (CLAUDE.md)
//! cargo build --workspace            # mirador, mira, fake-agent
//! ```

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use cmux_protocol::{Request, RequestEnvelope, ResponseEnvelope};
use serde_json::Value;

/// The value after `flag` in an agent's arguments.
pub fn flag(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

/// Whether a process whose whole command line matches `pattern` (a
/// `pgrep -f` regular expression) is running.
pub fn process_running(pattern: &str) -> bool {
    Command::new("pgrep").args(["-f", pattern]).output().is_ok_and(|o| o.status.success())
}

/// Returns from the test, saying why, unless `MIRA_E2E=1`.
#[macro_export]
macro_rules! e2e {
    () => {
        if !$crate::enabled() {
            eprintln!("skipped: set MIRA_E2E=1 to run the app end to end");
            return;
        }
    };
}

/// Whether end-to-end tests run: they start the app, so only when asked.
pub fn enabled() -> bool {
    std::env::var("MIRA_E2E").is_ok_and(|v| v == "1")
}

/// Returns from the test, saying why, unless `MIRA_E2E_LIVE=1` and the real
/// `claude` and `codex` are on the PATH. CI never sets it.
#[macro_export]
macro_rules! live {
    () => {
        if !$crate::live() {
            eprintln!("skipped: set MIRA_E2E_LIVE=1 to run the real agents, which spends real tokens");
            return;
        }
        for agent in ["claude", "codex"] {
            if $crate::on_path(agent).is_none() {
                eprintln!("skipped: no `{agent}` on the PATH for a live test");
                return;
            }
        }
        if $crate::claude_login().is_none() {
            eprintln!("skipped: no CLAUDE_CODE_OAUTH_TOKEN or ANTHROPIC_API_KEY for the live claude");
            return;
        }
        if !$crate::codex_auth().is_file() {
            eprintln!("skipped: no {} for the live codex", $crate::codex_auth().display());
            return;
        }
    };
}

/// Whether live tests run: they start the real agents, which costs money.
pub fn live() -> bool {
    std::env::var("MIRA_E2E_LIVE").is_ok_and(|v| v == "1")
}

/// The login a live Claude Code runs on, as the variable that carries it
/// and its value: a plan's token (`claude setup-token`) when there is one,
/// else an API key.
pub fn claude_login() -> Option<(&'static str, String)> {
    ["CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_API_KEY"]
        .into_iter()
        .find_map(|var| std::env::var(var).ok().filter(|v| !v.is_empty()).map(|v| (var, v)))
}

/// The person's own Codex login, which a live sandbox copies.
pub fn codex_auth() -> PathBuf {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".codex"));
    home.join("auth.json")
}

/// Where `name` is on this process's PATH.
pub fn on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(name)).find(|p| p.is_file())
}

/// The repository's root.
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..").canonicalize().unwrap()
}

/// `target/debug`, where the binaries under test are.
fn bin_dir() -> PathBuf {
    repo().join("src-tauri/target/debug")
}

fn bin(name: &str) -> PathBuf {
    let path = bin_dir().join(if cfg!(windows) { format!("{name}.exe") } else { name.to_string() });
    assert!(path.is_file(), "{} is missing: run `cargo build --workspace` first", path.display());
    path
}

/// The release bundle's `mirador` or `mira`, which a live sandbox runs: it
/// embeds `dist`, so it needs no frontend served.
fn release_bin(name: &str) -> PathBuf {
    let path = repo().join("src-tauri/target/release/bundle/macos/Mirador.app/Contents/MacOS").join(name);
    assert!(path.is_file(), "{} is missing: run `npm run tauri build` first", path.display());
    path
}

/// Serves `dist` on :1420, where a debug build loads its frontend, for the
/// life of the test process — uncached, like `scripts/serve-dist.py`, so
/// WebKit never runs a bundle from before the last build. If something
/// already serves there, it must be serving this repository's `dist`.
pub fn serve_frontend() {
    static STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    STARTED.get_or_init(|| {
        let dist = repo().join("dist");
        let index = std::fs::read(dist.join("index.html"))
            .unwrap_or_else(|_| panic!("{} has no index.html: run `npm run build` first", dist.display()));
        match std::net::TcpListener::bind(("127.0.0.1", 1420)) {
            Ok(listener) => {
                std::thread::spawn(move || {
                    for stream in listener.incoming().flatten() {
                        let dist = dist.clone();
                        std::thread::spawn(move || serve_file(stream, &dist));
                    }
                });
            }
            Err(_) => {
                let served = http_get("/index.html");
                assert!(
                    served.as_deref() == Some(&index[..]),
                    "something else is serving :1420, and not this repository's dist/"
                );
            }
        }
    });
}

fn serve_file(mut stream: std::net::TcpStream, dist: &Path) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    // Drain the headers.
    let mut line = String::new();
    while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
        line.clear();
    }
    let path = request.split_whitespace().nth(1).unwrap_or("/").split('?').next().unwrap_or("/");
    let path = if path == "/" { "/index.html" } else { path };
    let file = dist.join(path.trim_start_matches('/'));
    let inside = file.canonicalize().ok().is_some_and(|f| f.starts_with(dist.canonicalize().unwrap()));
    let (status, body) = match std::fs::read(&file) {
        Ok(body) if inside => ("200 OK", body),
        _ => ("404 Not Found", b"not found".to_vec()),
    };
    let kind = match file.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("css") => "text/css",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("woff2") => "font/woff2",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(&body);
}

fn http_get(path: &str) -> Option<Vec<u8>> {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", 1420)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(s, "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").ok()?;
    let mut out = Vec::new();
    s.read_to_end(&mut out).ok()?;
    let split = out.windows(4).position(|w| w == b"\r\n\r\n")?;
    Some(out[split + 4..].to_vec())
}

/// One Mirador instance's world: its own home, config and runtime
/// directories, a project directory to work in, and the log its fake
/// agents write.
pub struct Sandbox {
    pub root: PathBuf,
    pub home: PathBuf,
    pub config: PathBuf,
    pub runtime: PathBuf,
    /// A project directory: the first pane starts here (`defaultCwd`).
    pub project: PathBuf,
    agent_log: PathBuf,
    app_log: PathBuf,
    app: Option<Child>,
    launches: u32,
    /// Extra environment for the app, and so for its panes and agents.
    env: Vec<(String, String)>,
    /// The release bundle and the real agents, not the debug build and
    /// `fake-agent`.
    live: bool,
    /// The tail of every credential a live sandbox was given: no file
    /// holding one is left behind.
    secrets: Vec<String>,
}

impl Sandbox {
    /// A fresh sandbox named `name`. Nothing runs until [`Self::launch`].
    pub fn new(name: &str) -> Self {
        Self::create(name, false)
    }

    /// Like [`Self::new`], but running the release bundle, with the real
    /// `claude` and `codex` from this process's PATH: they log in and spend
    /// real tokens. Only behind [`live!`].
    pub fn live(name: &str) -> Self {
        let mut sb = Self::create(name, true);
        sb.sign_in();
        sb
    }

    /// Hands the real agents the person's logins — one of [`claude_login`]
    /// for Claude Code, a copy of the Codex login — and settles every question they
    /// ask on a first run, so they start straight to their prompt. Drop
    /// removes it all, even after a failure.
    fn sign_in(&mut self) {
        let (var, login) = claude_login().expect("a Claude login");
        let project = self.project.canonicalize().unwrap();
        let mut claude = serde_json::json!({
            "hasCompletedOnboarding": true,
            "projects": { project.to_string_lossy(): { "hasTrustDialogAccepted": true } },
        });
        if var == "ANTHROPIC_API_KEY" {
            // Claude Code asks once about a key from the environment, and
            // remembers the answer by the key's last 20 characters.
            claude["customApiKeyResponses"] = serde_json::json!({ "approved": [tail(&login)], "rejected": [] });
        }
        std::fs::write(self.home.join(".claude.json"), claude.to_string()).unwrap();
        // Only the one: given a key as well, Claude Code would bill the key.
        self.set_env(var, &login);
        // Or it installs a copy of itself, 250 MB, in the sandbox's home.
        self.set_env("DISABLE_AUTOUPDATER", "1");
        self.secrets.push(tail(&login));
        let auth: Value = serde_json::from_slice(&std::fs::read(codex_auth()).unwrap()).unwrap();
        strings(&auth, &mut |s| {
            if s.len() >= 20 {
                self.secrets.push(tail(s));
            }
        });
        let codex = self.home.join(".codex");
        std::fs::create_dir_all(&codex).unwrap();
        // A copy, never a link: Drop deletes it.
        std::fs::copy(codex_auth(), codex.join("auth.json")).unwrap();
        std::fs::write(
            codex.join("config.toml"),
            format!("check_for_update_on_startup = false\n\n[projects.{:?}]\ntrust_level = \"trusted\"\n", project),
        )
        .unwrap();
    }

    /// The processes working in this sandbox: its panes' shells and agents.
    fn processes(&self) -> Vec<String> {
        let Ok(root) = self.root.canonicalize() else { return Vec::new() };
        let Ok(out) = Command::new("lsof").args(["-a", "-d", "cwd", "-F", "pn"]).output() else { return Vec::new() };
        let mut pid = "";
        let mut pids = Vec::new();
        for line in std::str::from_utf8(&out.stdout).unwrap_or_default().lines() {
            if let Some(p) = line.strip_prefix('p') {
                pid = p;
            } else if line.strip_prefix('n').is_some_and(|n| Path::new(n).starts_with(&root)) {
                pids.push(pid.to_string());
            }
        }
        pids
    }

    /// Waits for the agents working in the sandbox to end after a quit,
    /// killing those left after a while: an agent writes on its way out —
    /// Claude Code its `.claude.json` and a backup of it — and one still
    /// going would put files back into a sandbox already deleted, logins
    /// and all. Anything else is neither waited for nor killed: a person's
    /// shell in a kept sandbox, say.
    fn wait_for_agents(&self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let agents = agents_among(&self.processes());
            if agents.is_empty() {
                return;
            }
            if Instant::now() >= deadline {
                let _ = Command::new("kill").arg("-9").args(&agents).status();
                std::thread::sleep(Duration::from_millis(500));
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Deletes every file in the sandbox holding a credential. Not just the
    /// two written: the agents copy them on — Claude Code backs up
    /// `.claude.json`, Codex snapshots the shell's environment, key and all,
    /// and logs its token — so only a search finds them all.
    fn forget_logins(&self) {
        if self.secrets.is_empty() {
            return;
        }
        fn walk(dir: &Path, secrets: &[String]) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                let Ok(kind) = entry.file_type() else { continue };
                // Never through a link: the agents in `bin` are the real ones.
                if kind.is_dir() {
                    walk(&path, secrets);
                } else if kind.is_file() && std::fs::read(&path).is_ok_and(|b| holds_any(&b, secrets)) {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        walk(&self.root, &self.secrets);
    }

    fn create(name: &str, live: bool) -> Self {
        // Short: a Unix socket path over ~104 bytes cannot be bound.
        let root = std::env::temp_dir().join(format!("me2e-{}-{name}", std::process::id()));
        let runtime = PathBuf::from(format!("/tmp/me2e{}{}", std::process::id() % 100_000, short(name)));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&runtime);
        let home = root.join("home");
        let config = root.join("config");
        let project = home.join("proj");
        let fakebin = root.join("bin");
        for dir in [&home, &config.join("mirador"), &runtime, &project, &fakebin, &home.join(".claude")] {
            std::fs::create_dir_all(dir).unwrap();
        }
        // An empty .zshrc: no dotfiles of the person running the tests, and
        // no zsh-newuser-install menu waiting for a key.
        std::fs::write(home.join(".zshrc"), "").unwrap();
        std::fs::write(
            config.join("mirador/mirador.json"),
            serde_json::json!({ "persistSessions": false, "defaultCwd": project }).to_string(),
        )
        .unwrap();
        #[cfg(unix)]
        for name in ["claude", "codex"] {
            let agent = if live {
                on_path(name).unwrap_or_else(|| panic!("no `{name}` on the PATH"))
            } else {
                bin("fake-agent")
            };
            std::os::unix::fs::symlink(agent, fakebin.join(name)).unwrap();
        }
        Sandbox {
            agent_log: root.join("agents.jsonl"),
            app_log: root.join("app.log"),
            root,
            home,
            config,
            runtime,
            project,
            app: None,
            launches: 0,
            env: Vec::new(),
            live,
            secrets: Vec::new(),
        }
    }

    /// The `mirador` or `mira` this sandbox runs.
    fn bin(&self, name: &str) -> PathBuf {
        if self.live {
            release_bin(name)
        } else {
            bin(name)
        }
    }

    /// Starts the app and waits for its socket to answer.
    pub fn launch(&mut self) {
        assert!(self.app.is_none(), "already running");
        if !self.live {
            serve_frontend();
        }
        let _ = std::fs::remove_file(self.config.join("mirador/socket.json"));
        self.launches += 1;
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&self.app_log).unwrap();
        let path = format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", self.root.join("bin").display());
        let child = Command::new(self.bin("mirador"))
            .env_clear()
            .env("PATH", path)
            .env("HOME", &self.home)
            .env("USER", std::env::var("USER").unwrap_or_default())
            .env("SHELL", "/bin/zsh")
            .env("TERM", "xterm-256color")
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("XDG_CONFIG_HOME", &self.config)
            .env("E2E_AGENT_LOG", &self.agent_log)
            .env("E2E_MIRA", self.bin("mira"))
            .envs(self.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .current_dir(&self.project)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .expect("start mirador");
        self.app = Some(child);
        self.wait_for("the app's socket", Duration::from_secs(60), || {
            self.request(Request::ListTabs).ok().map(|_| ())
        });
    }

    /// Quits the app, ending its panes, and waits for it to exit.
    pub fn quit(&mut self) {
        let Some(mut app) = self.app.take() else { return };
        let _ = self.request(Request::Quit { end_sessions: true });
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if app.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = app.kill();
        let _ = app.wait();
    }

    /// One request over the sandbox's own socket — found through its own
    /// discovery file, never the real app's.
    pub fn request(&self, req: Request) -> Result<Value, String> {
        let text = std::fs::read_to_string(self.config.join("mirador/socket.json")).map_err(|e| e.to_string())?;
        let disc: cmux_core::ipc::Discovery = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let stream = cmux_core::transport::connect(&disc.socket).map_err(|e| e.to_string())?;
        let mut line = serde_json::to_vec(&RequestEnvelope { id: Some(1), req }).unwrap();
        line.push(b'\n');
        let mut reader = BufReader::new(stream);
        reader.get_mut().write_all(&line).map_err(|e| e.to_string())?;
        let mut response = String::new();
        reader.read_line(&mut response).map_err(|e| e.to_string())?;
        let resp: ResponseEnvelope = serde_json::from_str(&response).map_err(|e| format!("bad response: {e}"))?;
        if resp.ok {
            Ok(resp.data.unwrap_or(Value::Null))
        } else {
            Err(resp.error.unwrap_or_default())
        }
    }

    /// Like [`Self::request`], for a request that must succeed.
    pub fn call(&self, req: Request) -> Value {
        let what = format!("{req:?}");
        self.request(req).unwrap_or_else(|e| self.fail(&format!("{what} failed: {e}")))
    }

    /// Polls `check` until it gives a value, or fails the test after
    /// `timeout` saying what it waited for — never a fixed sleep, which is
    /// either too short on a slow runner or slow everywhere.
    pub fn wait_for<T>(&self, what: &str, timeout: Duration, mut check: impl FnMut() -> Option<T>) -> T {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(v) = check() {
                return v;
            }
            if Instant::now() >= deadline {
                self.fail(&format!("timed out after {timeout:?} waiting for {what}"));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Waits for `pane`'s screen to pass `check`, failing with the screen
    /// it last showed — what an agent's blocking question looks like.
    pub fn wait_for_screen(&self, pane: &str, what: &str, timeout: Duration, check: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + timeout;
        loop {
            let screen = self.screen(pane).unwrap_or_default();
            if check(&screen) {
                return screen;
            }
            if Instant::now() >= deadline {
                self.fail(&format!("timed out after {timeout:?} waiting for {what}; {pane} shows:\n{screen}"));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Fails the test with `why`, and what the app and its agents logged.
    pub fn fail(&self, why: &str) -> ! {
        let tail = |p: &Path| {
            std::fs::read_to_string(p)
                .map(|t| t.lines().rev().take(30).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"))
                .unwrap_or_default()
        };
        let mut message = format!(
            "{why}\n--- app log ({}) ---\n{}\n--- agents ---\n{}",
            self.app_log.display(),
            tail(&self.app_log),
            tail(&self.agent_log)
        );
        // A screen can show a credential: Claude Code asking about a key
        // shows its end.
        for secret in &self.secrets {
            message = message.replace(secret.as_str(), "[credential]");
        }
        panic!("{message}");
    }

    /// The workspace snapshot.
    pub fn snapshot(&self) -> Value {
        self.call(Request::ListTabs)
    }

    /// Every pane id, tab by tab, in order.
    pub fn panes(&self) -> Vec<String> {
        fn leaves(node: &Value, out: &mut Vec<String>) {
            if let Some(p) = node["paneId"].as_str() {
                out.push(p.to_string());
            }
            for c in node["children"].as_array().into_iter().flatten() {
                leaves(c, out);
            }
        }
        let mut out = Vec::new();
        for tab in self.snapshot()["tabs"].as_array().into_iter().flatten() {
            leaves(&tab["root"], &mut out);
        }
        out
    }

    /// What `pane` shows; `None` while it cannot say (not mounted yet).
    pub fn screen(&self, pane: &str) -> Option<String> {
        self.request(Request::ReadScreen { pane_id: Some(pane.to_string()), lines: None })
            .ok()
            .and_then(|v| v["text"].as_str().map(String::from))
    }

    /// Types `text` into `pane`.
    pub fn type_into(&self, pane: &str, text: &str) {
        self.call(Request::SendInput { pane_id: Some(pane.to_string()), data: text.to_string() });
    }

    /// Everything the fake agents have logged so far, oldest first. A line
    /// that does not parse fails the test: skipping it once hid two agents'
    /// lines written into each other, and with them an agent's start.
    pub fn agent_events(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.agent_log)
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap_or_else(|e| self.fail(&format!("agent log line {l:?}: {e}"))))
            .collect()
    }

    /// Sets `key` in the app's environment from the next launch on — its
    /// panes', and so the fake agents', too.
    pub fn set_env(&mut self, key: &str, value: &str) {
        self.env.retain(|(k, _)| k != key);
        self.env.push((key.to_string(), value.to_string()));
    }

    /// Writes `text` to `path`, relative to the project directory.
    pub fn write(&self, path: &str, text: &str) {
        let path = self.project.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// The real `mira` CLI as if run from `pane` (its `MIRA_PANE`),
    /// against this sandbox, not yet started.
    pub fn mira_command(&self, pane: Option<&str>, args: &[&str]) -> Command {
        let mut cmd = Command::new(self.bin("mira"));
        cmd.args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &self.home)
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("XDG_CONFIG_HOME", &self.config);
        if let Some(pane) = pane {
            cmd.env("MIRA_PANE", pane);
        }
        cmd
    }

    /// Runs [`Self::mira_command`]: exit code, stdout, stderr.
    pub fn mira(&self, pane: Option<&str>, args: &[&str]) -> (i32, String, String) {
        let out = self.mira_command(pane, args).output().expect("run mira");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    /// The first pane, once its terminal is drawn.
    pub fn first_pane(&self) -> String {
        let pane = self.wait_for("a pane", Duration::from_secs(45), || self.panes().into_iter().next());
        self.wait_for("the first pane to mount", Duration::from_secs(45), || self.screen(&pane));
        pane
    }

    /// The fake agents' events of one kind in `pane`, oldest first.
    pub fn events(&self, event: &str, pane: &str) -> Vec<Value> {
        self.agent_events()
            .into_iter()
            .filter(|e| e["event"] == event && e["pane"] == pane)
            .collect()
    }

    /// Waits for the `n`th (from 1) agent started in `pane`, and gives its
    /// arguments.
    pub fn nth_start(&self, pane: &str, n: usize) -> Vec<String> {
        let start = self.wait_for(&format!("agent start #{n} in {pane}"), Duration::from_secs(45), || {
            self.events("start", pane).into_iter().nth(n - 1)
        });
        serde_json::from_value(start["args"].clone()).unwrap()
    }

    /// The running teams, as `team list` gives them.
    pub fn teams(&self) -> Vec<Value> {
        self.call(Request::TeamList)["teams"].as_array().cloned().unwrap_or_default()
    }

    /// One team member, by name, from `team list`.
    pub fn member(&self, name: &str) -> Option<Value> {
        self.teams()
            .into_iter()
            .flat_map(|t| t["members"].as_array().cloned().unwrap_or_default())
            .find(|m| m["name"] == name)
    }

    /// How many times this sandbox has been launched.
    pub fn launches(&self) -> u32 {
        self.launches
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        self.quit();
        self.wait_for_agents();
        let _ = std::fs::remove_dir_all(&self.runtime);
        // A failed test leaves its directory for a look — but not the
        // logins in it; a passing one leaves nothing.
        if std::thread::panicking() {
            self.forget_logins();
        } else {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

/// Which of `pids` are `claude` or `codex`, by `ps`'s name for them —
/// not `lsof`'s, which names Claude Code by its versioned binary
/// (`2.1.302`).
fn agents_among(pids: &[String]) -> Vec<String> {
    if pids.is_empty() {
        return Vec::new();
    }
    let Ok(out) = Command::new("ps").args(["-o", "pid=,comm=", "-p", &pids.join(",")]).output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let pid = words.next()?;
            let name = Path::new(words.next()?).file_name()?.to_str()?;
            matches!(name, "claude" | "codex").then(|| pid.to_string())
        })
        .collect()
}

/// The last 20 characters of a credential: enough to find it, and how
/// Claude Code remembers an approved key.
fn tail(secret: &str) -> String {
    secret.chars().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect()
}

/// Every string in `value`, however deep.
fn strings(value: &Value, f: &mut impl FnMut(&str)) {
    match value {
        Value::String(s) => f(s),
        Value::Array(a) => a.iter().for_each(|v| strings(v, f)),
        Value::Object(o) => o.values().for_each(|v| strings(v, f)),
        _ => {}
    }
}

/// Whether `bytes` holds any of `secrets`, in one pass: the sandbox it
/// searches is tens of megabytes, and this runs unoptimized.
fn holds_any(bytes: &[u8], secrets: &[String]) -> bool {
    (0..bytes.len()).any(|i| secrets.iter().any(|s| bytes[i..].starts_with(s.as_bytes())))
}

fn short(name: &str) -> String {
    name.chars().filter(|c| c.is_ascii_alphanumeric()).take(6).collect()
}
