//! End-to-end tests: the real Mirador, started in a throwaway sandbox and
//! driven over its socket, the way a person checks a change by hand (see
//! CLAUDE.md, *Verifying a change*) — written down, so it runs again.
//!
//! A [`Sandbox`] is a home directory, config directory and runtime
//! directory of its own: its socket, session, scrollback and prompts never
//! touch the real app's, and its shell reads an empty `.zshrc`. Agents in
//! it are `fake-agent` linked in as `claude` and `codex`, so a test needs
//! no login and spends nothing, and can make an agent do what it needs.
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
}

impl Sandbox {
    /// A fresh sandbox named `name`. Nothing runs until [`Self::launch`].
    pub fn new(name: &str) -> Self {
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
            std::os::unix::fs::symlink(bin("fake-agent"), fakebin.join(name)).unwrap();
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
        }
    }

    /// Starts the app and waits for its socket to answer.
    pub fn launch(&mut self) {
        assert!(self.app.is_none(), "already running");
        serve_frontend();
        let _ = std::fs::remove_file(self.config.join("mirador/socket.json"));
        self.launches += 1;
        let log = std::fs::OpenOptions::new().create(true).append(true).open(&self.app_log).unwrap();
        let path = format!("{}:/usr/bin:/bin:/usr/sbin:/sbin", self.root.join("bin").display());
        let child = Command::new(bin("mirador"))
            .env_clear()
            .env("PATH", path)
            .env("HOME", &self.home)
            .env("USER", std::env::var("USER").unwrap_or_default())
            .env("SHELL", "/bin/zsh")
            .env("TERM", "xterm-256color")
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("XDG_CONFIG_HOME", &self.config)
            .env("E2E_AGENT_LOG", &self.agent_log)
            .env("E2E_MIRA", bin("mira"))
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

    /// Fails the test with `why`, and what the app and its agents logged.
    pub fn fail(&self, why: &str) -> ! {
        let tail = |p: &Path| {
            std::fs::read_to_string(p)
                .map(|t| t.lines().rev().take(30).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"))
                .unwrap_or_default()
        };
        panic!(
            "{why}\n--- app log ({}) ---\n{}\n--- agents ---\n{}",
            self.app_log.display(),
            tail(&self.app_log),
            tail(&self.agent_log)
        );
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
        let mut cmd = Command::new(bin("mira"));
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

    /// How many times this sandbox has been launched.    /// How many times this sandbox has been launched.
    pub fn launches(&self) -> u32 {
        self.launches
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        self.quit();
        let _ = std::fs::remove_dir_all(&self.runtime);
        // A failed test leaves its directory for a look; a passing one not.
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

fn short(name: &str) -> String {
    name.chars().filter(|c| c.is_ascii_alphanumeric()).take(6).collect()
}
