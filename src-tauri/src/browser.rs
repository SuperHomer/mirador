//! Browser panes: native child webviews (Tauri multiwebview, `unstable`)
//! positioned over pane rects. Automation (snapshot/click/fill/eval) runs
//! through an injected script, whose return value Rust reads back with
//! `eval_with_callback` — remote pages get no IPC access to the app.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, Url, Webview, WebviewUrl};

use crate::AppState;

const BRIDGE_JS: &str = include_str!("browser_bridge.js");

#[derive(Default)]
pub struct BrowserBridge {
    next_id: AtomicU64,
    /// Memoized host-view origin. Cleared by `forget_host_origin` when the
    /// window moves or resizes, which is the only time it can change.
    host_origin: Mutex<Option<(f64, f64)>>,
    /// Panes whose webview has finished loading a page at least once.
    loaded: Mutex<HashSet<String>>,
    /// Panes whose first page did not load in `FIRST_LOAD_WAIT`: a command
    /// to one says so at once, rather than waiting again.
    not_loaded: Mutex<HashSet<String>>,
    /// Each pane's page loads, counted, so a command can wait for the one
    /// it started rather than any.
    loads: Mutex<HashMap<String, Loads>>,
    /// Browser panes by when they were last opened or driven, latest last:
    /// the pane a command goes to when it names none.
    recent: Mutex<Vec<String>>,
    /// Held while `ensure_webview` looks for a pane's webview and creates
    /// it: a pane reports its bounds two or three times as it mounts, and
    /// those calls run at once, each finding no webview yet.
    creating: Mutex<()>,
    /// Whether the frontend last asked for each pane's page to be shown.
    /// A request can come before the webview exists, and the frontend asks
    /// only when that changes, so creation applies it.
    shown: Mutex<HashMap<String, bool>>,
}

impl BrowserBridge {
    /// Marks `pane` as the browser pane opened or driven last.
    pub fn used(&self, pane: &str) {
        let mut recent = self.recent.lock().unwrap();
        recent.retain(|p| p != pane);
        recent.push(pane.to_string());
    }

    /// The one of `panes` opened or driven last, else the first.
    pub fn latest_of(&self, panes: &[String]) -> Option<String> {
        let recent = self.recent.lock().unwrap();
        recent.iter().rev().find(|p| panes.contains(p)).or(panes.first()).cloned()
    }
}

/// A pane's page loads, by count: navigations asked for (`on_navigation`,
/// which on macOS counts the page's frames too), new documents committed
/// (`PageLoadEvent::Started`), and, of those, the latest one finished.
#[derive(Clone, Copy, Default)]
pub struct Loads {
    asked: u64,
    started: u64,
    finished: u64,
}

/// How long a navigation command waits for anything to start loading
/// before taking it that nothing will: back with no history, a pushState
/// entry. Generous, for a slow machine; it is spent only on those.
const NAVIGATION_START_WAIT: Duration = Duration::from_secs(2);

/// How long a command waits for the page to answer.
const ANSWER_WAIT: Duration = Duration::from_secs(10);
/// The most a page's answer may be, as the callback string carries it.
/// The bridge holds itself to the same (`MAX_ANSWER_BYTES` in
/// browser_bridge.js), but the page controls that script: this is the
/// limit that holds.
const MAX_ANSWER_BYTES: usize = 1_000_000;

/// How long a command waits for a browser pane's webview to exist.
const WEBVIEW_WAIT: Duration = Duration::from_secs(5);
/// How long `open` waits for its first page, past which it returns anyway:
/// the pane is open, the page is just slow (or never answers).
const FIRST_LOAD_WAIT: Duration = Duration::from_secs(10);

fn label_for(pane_id: &str) -> String {
    let safe: String = pane_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    format!("browser-{safe}")
}

/// `url` as typed, made a URL against the page the pane shows now
/// (`cmux_core::browser_url::resolve`).
/// Refused when it is not a web page, or is one of Mirador's own
/// (`cmux_core::pane_origin`).
pub fn parse_url(app: &AppHandle, url: &str, current: Option<&str>) -> Result<Url, String> {
    let parsed: Url = cmux_core::browser_url::resolve(url, current)?
        .parse()
        .map_err(|e| format!("bad url {url}: {e}"))?;
    cmux_core::pane_origin::may_open(parsed.as_str(), dev_origin(app).as_deref())?;
    Ok(parsed)
}

/// Where a debug build loads its frontend from: an origin of the app's
/// own, as much as `tauri://localhost` is in a release build.
fn dev_origin(app: &AppHandle) -> Option<String> {
    if !tauri::is_dev() {
        return None;
    }
    app.config().build.dev_url.as_ref().map(|u| u.to_string())
}

/// Where the host webview's client area starts inside the window.
///
/// A child webview is positioned against the window, but the frontend
/// measures a pane's placeholder in host-webview viewport coordinates. The
/// two differ by wherever the host webview itself begins: nothing when it
/// fills the frame, a titlebar's worth when it does not. Asking the host
/// for its own position reads that gap out of the very coordinate space
/// `set_position` writes into, so there is no per-platform titlebar
/// arithmetic to get wrong.
///
/// This replaces deriving the gap from `window.screenY` in the frontend.
/// That is not a real coordinate in a wry webview: it reports no window
/// rect (`outerWidth`/`outerHeight` read back as 0), so `screenY` comes out
/// as the screen height — ~870px of bogus offset, which pushed every
/// browser page off the bottom of the window and made the panes look empty.
fn host_origin(app: &AppHandle, window: &tauri::Window) -> (f64, f64) {
    if let Some(cached) = *app.state::<AppState>().browser_bridge.host_origin.lock().unwrap() {
        return cached;
    }
    let measured = measure_host_origin(window);
    *app.state::<AppState>()
        .browser_bridge
        .host_origin
        .lock()
        .unwrap() = Some(measured);
    measured
}

/// Drops the memoized origin. The host view can only move within the window
/// when the window itself changes — a resize, a screen change, entering or
/// leaving fullscreen — so those events are what invalidate it.
pub fn forget_host_origin(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        *state.browser_bridge.host_origin.lock().unwrap() = None;
    }
}

fn measure_host_origin(window: &tauri::Window) -> (f64, f64) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let host = window
        .webviews()
        .into_iter()
        .find(|w| !w.label().starts_with("browser-"));
    let Some(pos) = host.and_then(|w| w.position().ok()) else {
        return (0.0, 0.0);
    };
    let (x, y) = (f64::from(pos.x) / scale, f64::from(pos.y) / scale);
    // An inset is a window decoration at most. Anything bigger means the
    // coordinate space is not the one assumed here, and keeping pages on
    // their pane matters more than honouring it.
    if (0.0..=200.0).contains(&x) && (0.0..=200.0).contains(&y) {
        (x, y)
    } else {
        (0.0, 0.0)
    }
}

/// Creates the pane's child webview if needed and applies bounds. Never
/// from a sync command: on Windows creating a webview there deadlocks
/// (see `commands::set_browser_bounds`).
pub fn ensure_webview(
    app: &AppHandle,
    pane_id: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), String> {
    let label = label_for(pane_id);
    let window = app
        .get_window("main")
        .ok_or_else(|| "main window missing".to_string())?;
    let (origin_x, origin_y) = host_origin(app, &window);
    let (x, y) = (x + origin_x, y + origin_y);

    let state = app.state::<AppState>();
    let _creating = state.browser_bridge.creating.lock().unwrap();
    if let Some(webview) = app.get_webview(&label) {
        let _ = webview.set_position(LogicalPosition::new(x, y));
        let _ = webview.set_size(LogicalSize::new(w.max(1.0), h.max(1.0)));
        return Ok(());
    }

    let url = {
        let meta = state.meta.lock().unwrap();
        meta.get(pane_id)
            .and_then(|m| m.browser_url.clone())
            .ok_or_else(|| format!("no browser pane {pane_id}"))?
    };

    let nav_pane = pane_id.to_string();
    let nav_app = app.clone();
    let nav_pane_asked = pane_id.to_string();
    let dev = dev_origin(app);
    let builder = tauri::webview::WebviewBuilder::new(&label, WebviewUrl::External(parse_url(app, &url, None)?))
        .initialization_script(
            BRIDGE_JS
                .replace("__MIRA_WEBVIEW_LABEL__", &label)
                // tauri-plugin-notification's script asks for its permission
                // on load everywhere but Windows (its `__TEMPLATE_windows__`).
                .replace("__MIRA_AMBIENT_CHECK__", if cfg!(windows) { "false" } else { "true" }),
        )
        // What the page itself asks for — a link, a script, a redirect —
        // never reaches the app's own origins either. Only those are
        // refused: on macOS this sees the page's frames too, which may
        // well load `about:srcdoc`, `blob:` or `data:`.
        .on_navigation(move |url| {
            if cmux_core::pane_origin::is_app_origin(url.as_str(), dev.as_deref()) {
                return false;
            }
            count_load(&nav_app, &nav_pane_asked, |l| l.asked += 1);
            true
        })
        // The page that loaded, not the one asked for: a navigation that
        // fails (nothing listening, a bad certificate) never finishes, and
        // the pane goes on showing — and reporting — the page before it.
        .on_page_load(move |webview, payload| {
            if payload.event() == PageLoadEvent::Started {
                count_load(webview.app_handle(), &nav_pane, |l| l.started += 1);
            }
            if payload.event() == PageLoadEvent::Finished {
                let app = webview.app_handle();
                count_load(app, &nav_pane, |l| l.finished = l.started);
                let bridge = &app.state::<AppState>().browser_bridge;
                bridge.loaded.lock().unwrap().insert(nav_pane.clone());
                bridge.not_loaded.lock().unwrap().remove(&nav_pane);
                track_navigation(app, &nav_pane, payload.url());
            }
        });

    // Built and its visibility settled in one main-thread task, as the sync
    // command used to: a page hidden only by a later task could paint at its
    // place in between, over the board. `add_child` runs inline there (Tauri
    // runs a main-thread task in place when already on it), and nothing is
    // drawn until the task returns. `set_visible` and `destroy` run on the
    // main thread too, so neither lands between the checks and the build.
    let (tx, rx) = mpsc::channel();
    let main_app = app.clone();
    let main_pane = pane_id.to_string();
    let position = LogicalPosition::new(x, y);
    let size = LogicalSize::new(w.max(1.0), h.max(1.0));
    app.run_on_main_thread(move || {
        let _ = tx.send(create_on_main_thread(&main_app, &window, &main_pane, builder, position, size));
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

fn create_on_main_thread(
    app: &AppHandle,
    window: &tauri::Window,
    pane_id: &str,
    builder: tauri::webview::WebviewBuilder<tauri::Wry>,
    position: LogicalPosition<f64>,
    size: LogicalSize<f64>,
) -> Result<(), String> {
    // A bounds call the frontend sent before the pane closed can get here
    // after it, and would make a webview nobody owns.
    if !is_browser_pane(app, pane_id) {
        return Err(format!("no browser pane {pane_id}"));
    }
    let webview = window.add_child(builder, position, size).map_err(|e| e.to_string())?;
    let state = app.state::<AppState>();
    let asked = state.browser_bridge.shown.lock().unwrap().get(pane_id).copied();
    let board = state.workspace.lock().unwrap().view == cmux_protocol::View::Board;
    if hidden_when_created(asked, board) {
        let _ = webview.hide();
    }
    Ok(())
}

/// Whether a just-created webview starts hidden: as the frontend last
/// asked, when it has. Otherwise, on the board a page is shown only once
/// its card can show it (at 100%, in view, the board still), which the
/// board decides and asks for. A pane opened there must not appear over
/// the board until then.
fn hidden_when_created(asked: Option<bool>, board: bool) -> bool {
    match asked {
        Some(shown) => !shown,
        None => board,
    }
}

fn is_browser_pane(app: &AppHandle, pane_id: &str) -> bool {
    let state = app.state::<AppState>();
    let meta = state.meta.lock().unwrap();
    meta.get(pane_id).is_some_and(|m| m.browser_url.is_some())
}

fn count_load(app: &AppHandle, pane_id: &str, count: impl FnOnce(&mut Loads)) {
    let state = app.state::<AppState>();
    count(state.browser_bridge.loads.lock().unwrap().entry(pane_id.to_string()).or_default());
}

/// The pane's load counts now.
fn loads(app: &AppHandle, pane_id: &str) -> Loads {
    let state = app.state::<AppState>();
    let loads = state.browser_bridge.loads.lock().unwrap();
    loads.get(pane_id).copied().unwrap_or_default()
}

/// Waits for the page a navigation started after `before` to finish
/// loading, so the next command finds it and not the page before it: on a
/// slow machine the old document goes on answering for a while. Waits
/// `FIRST_LOAD_WAIT` at most, then answers with a note instead of hanging
/// (a navigation that failed never finishes). When nothing asks to load
/// within `NAVIGATION_START_WAIT` — back with nowhere to go, a pushState
/// entry — nothing will, and it answers then.
pub fn wait_for_new_page(app: &AppHandle, pane_id: &str, before: Loads) -> Option<String> {
    let start = Instant::now();
    loop {
        let now = loads(app, pane_id);
        // `finished` holds the count of the load that finished: one past
        // `before.started` is a document committed after the command.
        if now.finished > before.started {
            return None;
        }
        let idle = now.asked == before.asked && now.started == before.started;
        if idle && start.elapsed() >= NAVIGATION_START_WAIT {
            return None;
        }
        if start.elapsed() >= FIRST_LOAD_WAIT {
            return Some(format!(
                "the page had not finished loading after {}s (did it fail to load?)",
                FIRST_LOAD_WAIT.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn track_navigation(app: &AppHandle, pane_id: &str, url: &Url) {
    let state = app.state::<AppState>();
    let changed = record_navigation(&mut state.meta.lock().unwrap(), pane_id, url);
    if changed {
        crate::commands::emit_workspace(app);
    }
}

/// Sets a browser pane's URL; true when it changed. A page finishing in a
/// webview being closed must not bring its pane back, so a pane with no
/// meta, or one that is not a browser pane, is left as it is.
fn record_navigation(meta: &mut HashMap<String, cmux_core::state::PaneMeta>, pane_id: &str, url: &Url) -> bool {
    let Some(entry) = meta.get_mut(pane_id).filter(|m| m.browser_url.is_some()) else {
        return false;
    };
    let new = Some(url.to_string());
    if entry.browser_url == new {
        return false;
    }
    entry.browser_url = new;
    true
}

/// The pane's webview, waiting a moment for it: the frontend creates it
/// when it first draws the pane, just after `open` has answered. Never
/// call it on the main thread, which is the one that creates it.
///
/// Gives up at `deadline` too, when there is one sooner: `wait --timeout
/// 1` means one second, the wait for a pane to be drawn included.
fn webview(app: &AppHandle, pane_id: &str, deadline: Option<Instant>) -> Result<Webview, String> {
    if !is_browser_pane(app, pane_id) {
        return Err(format!("no browser pane {pane_id}"));
    }
    let own = Instant::now() + WEBVIEW_WAIT;
    let deadline = deadline.map_or(own, |d| d.min(own));
    loop {
        if let Some(webview) = app.get_webview(&label_for(pane_id)) {
            return Ok(webview);
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "browser pane {pane_id} has no page yet: the app has not drawn it. \
                 Is Mirador's window open? Show the pane's tab and try again"
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Waits for a just-opened pane's webview, and then for its first page
/// to load or `FIRST_LOAD_WAIT`, whichever comes first — so the command
/// after `open` finds the page there.
pub fn wait_for_first_load(app: &AppHandle, pane_id: &str) -> Result<(), String> {
    webview(app, pane_id, None)?;
    // A page that never loads still leaves an open pane: `open` succeeded.
    let _ = first_load(app, pane_id, None);
    Ok(())
}

/// Waits for the pane's first page to have loaded. Until one has, the
/// bridge is not in it — a failed load shows WebKit's error page, which
/// would leave every command to time out as if the page were busy.
///
/// Gives up at `deadline` when there is one, the caller's — `wait
/// --timeout 1` means one second, first load included — but only a full
/// `FIRST_LOAD_WAIT` marks the page as one that never loaded.
fn first_load(app: &AppHandle, pane_id: &str, deadline: Option<Instant>) -> Result<(), String> {
    let state = app.state::<AppState>();
    let bridge = &state.browser_bridge;
    let given_up = Instant::now() + FIRST_LOAD_WAIT;
    let url = || state.meta.lock().unwrap().get(pane_id).and_then(|m| m.browser_url.clone()).unwrap_or_default();
    loop {
        if bridge.loaded.lock().unwrap().contains(pane_id) {
            return Ok(());
        }
        if bridge.not_loaded.lock().unwrap().contains(pane_id) || Instant::now() >= given_up {
            bridge.not_loaded.lock().unwrap().insert(pane_id.to_string());
            return Err(format!(
                "the page at {} never loaded: is anything serving it? `mira browser navigate` to another",
                url()
            ));
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(format!("the page at {} has not loaded yet", url()));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Runs an automation op inside the pane's page and gives its JSON
/// answer, waiting up to `ANSWER_WAIT` — for a Promise `eval` too.
pub fn execute(app: &AppHandle, pane_id: &str, op: serde_json::Value) -> Result<String, String> {
    execute_by(app, pane_id, op, None)
}

/// `execute`, all of it — the first load too — over by `deadline` when
/// there is one.
fn execute_by(app: &AppHandle, pane_id: &str, op: serde_json::Value, deadline: Option<Instant>) -> Result<String, String> {
    let webview = webview(app, pane_id, deadline)?;
    first_load(app, pane_id, deadline)?;
    let deadline = deadline.unwrap_or_else(|| Instant::now() + ANSWER_WAIT);
    let id = app.state::<AppState>().browser_bridge.next_id.fetch_add(1, Ordering::Relaxed);
    let op_json = serde_json::to_string(&op).map_err(|e| e.to_string())?;
    let mut answer = ask(&webview, format!("window.__miraRun ? window.__miraRun({id}, {op_json}) : null"), deadline)?;
    // A Promise: its answer is collected once it settles.
    while serde_json::from_str::<serde_json::Value>(&answer).is_ok_and(|v| v["pending"] == true) {
        // A page can answer "pending" forever: the deadline holds for all.
        if Instant::now() >= deadline {
            return Err(TIMED_OUT.into());
        }
        std::thread::sleep(Duration::from_millis(20));
        answer = ask(&webview, format!("window.__miraTake ? window.__miraTake({id}) : null"), deadline)?;
    }
    Ok(answer)
}

const TIMED_OUT: &str = "browser automation timed out: the page is busy, or still waiting on a Promise";

/// Evaluates `js` in the page and gives back the JSON text it returned.
fn ask(webview: &Webview, js: String, deadline: Instant) -> Result<String, String> {
    let (tx, rx) = mpsc::channel();
    webview
        .eval_with_callback(js, move |returned| {
            let _ = tx.send(returned);
        })
        .map_err(|e| e.to_string())?;
    let returned = rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| TIMED_OUT.to_string())?;
    // Before parsing: a hostile page can replace `__miraRun` and answer
    // anything, at any size. The engine has already built the string by
    // now, so this does not bound that first allocation; it bounds the
    // parse and everything after it, the socket and the CLI included.
    if returned.len() > MAX_ANSWER_BYTES {
        return Err(format!("result too large ({} bytes, max {MAX_ANSWER_BYTES})", returned.len()));
    }
    // What the script returned, as JSON: a string, holding the answer's own
    // JSON. Anything else means the bridge was not there to call.
    serde_json::from_str::<Option<String>>(&returned)
        .ok()
        .flatten()
        .ok_or_else(|| "the page has no automation bridge (a browser error page?)".to_string())
}

/// What `wait` waits for.
pub enum Until {
    /// An element matching the selector, shown.
    Shown(String),
    /// The page's visible text containing this.
    Text(String),
    /// The loaded page's URL containing this.
    Url(String),
    /// Nothing matching the selector shown.
    Gone(String),
}

impl std::fmt::Display for Until {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Until::Shown(s) => write!(f, "{s} to be shown"),
            Until::Text(t) => write!(f, "the text {t:?}"),
            Until::Url(u) => write!(f, "a URL containing {u:?}"),
            Until::Gone(s) => write!(f, "{s} to be gone"),
        }
    }
}

/// Waits until the page meets `until`, asking it again every 100 ms.
///
/// The polling is Rust's, not a Promise in the page: a navigation or a
/// reload starts a new document, which would drop a Promise waiting in
/// the old one, and the wait may outlast `ANSWER_WAIT`. A failed ask is
/// one more try — mid-navigation the page cannot answer — and the last
/// one's error says why, on a timeout.
pub fn wait(app: &AppHandle, pane_id: &str, until: &Until, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let op = match until {
        Until::Shown(s) => serde_json::json!({ "op": "check", "shown": s }),
        Until::Text(t) => serde_json::json!({ "op": "check", "text": t }),
        Until::Gone(s) => serde_json::json!({ "op": "check", "gone": s }),
        Until::Url(_) => serde_json::Value::Null,
    };
    loop {
        let last = if let Until::Url(part) = until {
            // The URL the pane reports is the page that finished loading.
            let state = app.state::<AppState>();
            let url = state.meta.lock().unwrap().get(pane_id).and_then(|m| m.browser_url.clone());
            let url = url.unwrap_or_default();
            if url.contains(part.as_str()) {
                return Ok(());
            }
            format!("the page is at {url}")
        } else {
            // At least a second per look, even the last: one cut short by
            // the deadline would time out, and its error would replace the
            // glimpse of the page the timeout message shows.
            let left = deadline.saturating_duration_since(Instant::now());
            let ask_by = Instant::now() + left.clamp(Duration::from_secs(1), ANSWER_WAIT);
            match execute_by(app, pane_id, op.clone(), Some(ask_by)) {
                Ok(raw) => {
                    let answer: serde_json::Value =
                        serde_json::from_str(&raw).map_err(|e| format!("bad bridge payload: {e}"))?;
                    // The page refused the condition itself (a bad selector):
                    // no amount of waiting fixes that.
                    if let Some(error) = answer["error"].as_str() {
                        return Err(error.to_string());
                    }
                    if answer["met"] == true {
                        return Ok(());
                    }
                    answer["shows"].as_str().unwrap_or_default().to_string()
                }
                Err(e) => e,
            }
        };
        if Instant::now() >= deadline {
            return Err(format!("timed out after {}s waiting for {until}; {last}", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Sends the pane to `url`. Gives the load counts from just before, to
/// hand `wait_for_new_page` — or `None` when it loads no new page, as a
/// `#fragment` of the current one does not.
pub fn navigate(app: &AppHandle, pane_id: &str, url: &str) -> Result<Option<Loads>, String> {
    let webview = webview(app, pane_id, None)?;
    // The page the pane reports, not `webview.url()`: wry unwraps the
    // webview's URL, which is nil after a first load that failed, and the
    // app panics.
    let current = {
        let state = app.state::<AppState>();
        let meta = state.meta.lock().unwrap();
        meta.get(pane_id).and_then(|m| m.browser_url.clone())
    };
    let parsed = parse_url(app, url, current.as_deref())?;
    let same_document = parsed.fragment().is_some()
        && current.as_deref().and_then(|c| c.parse::<Url>().ok()).is_some_and(|mut c| {
            c.set_fragment(None);
            let mut target = parsed.clone();
            target.set_fragment(None);
            c == target
        });
    // Counted after the webview is found, which can take a while: a load
    // finishing during that wait is not the one this starts.
    let before = loads(app, pane_id);
    webview.navigate(parsed).map_err(|e| e.to_string())?;
    Ok((!same_document).then_some(before))
}

/// Goes back, forward or reloads; gives the load counts from just before,
/// as `navigate` does.
pub fn history(app: &AppHandle, pane_id: &str, action: &str) -> Result<Loads, String> {
    let webview = webview(app, pane_id, None)?;
    let js = match action {
        "back" => "history.back()",
        "forward" => "history.forward()",
        "reload" => "location.reload()",
        _ => return Err(format!("unknown history action {action}")),
    };
    let before = loads(app, pane_id);
    webview.eval(js).map_err(|e| e.to_string())?;
    Ok(before)
}

/// The pane's page as a PNG: the part of it the pane shows, at the
/// display's resolution. A hidden page is captured as it is now, not as
/// it last showed — on the board, in another tab, the window minimised.
pub fn screenshot(app: &AppHandle, pane_id: &str) -> Result<Vec<u8>, String> {
    let webview = webview(app, pane_id, None)?;
    // A page that never loads is captured as it shows: the error page.
    let _ = first_load(app, pane_id, None);
    capture(&webview, Instant::now() + ANSWER_WAIT)
}

const CAPTURE_TIMED_OUT: &str = "the browser did not capture the page in time";

/// WebKit's snapshot is taken on the main thread, which WebKit requires,
/// and it answers there; only the image is taken there. Drawing it into
/// pixels and encoding the PNG, 30–50 ms for a pane, happen on this
/// thread instead, so the app does not stall for them.
#[cfg(target_os = "macos")]
fn capture(webview: &Webview, deadline: Instant) -> Result<Vec<u8>, String> {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSImage;
    use objc2_core_graphics::CGImage;
    use objc2_foundation::NSError;
    use objc2_web_kit::{WKSnapshotConfiguration, WKWebView};

    let (tx, rx) = mpsc::channel::<Result<Retained<CGImage>, String>>();
    webview
        .with_webview(move |platform| {
            let mtm = MainThreadMarker::new().expect("with_webview runs on the main thread");
            // SAFETY: on macOS the platform webview is the WKWebView, alive
            // for this call.
            let wk: &WKWebView = unsafe { &*platform.inner().cast() };
            let done = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                // SAFETY: WebKit hands a valid image, or null and an error.
                let image = match unsafe { (image.as_ref(), error.as_ref()) } {
                    (Some(image), _) => unsafe {
                        image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None)
                    }
                    .ok_or_else(|| "the page's snapshot has no image".to_string()),
                    (None, Some(error)) => Err(format!("the browser could not capture the page: {}", error.localizedDescription())),
                    (None, None) => Err("the browser could not capture the page".to_string()),
                };
                let _ = tx.send(image);
            });
            unsafe {
                let config = WKSnapshotConfiguration::new(mtm);
                wk.takeSnapshotWithConfiguration_completionHandler(Some(&config), &done);
            }
        })
        .map_err(|e| e.to_string())?;
    let image = rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| CAPTURE_TIMED_OUT.to_string())??;
    encode_png(&image)
}

/// The image drawn into sRGB pixels, its alpha dropped — a page is
/// opaque — and encoded.
#[cfg(target_os = "macos")]
fn encode_png(image: &objc2_core_graphics::CGImage) -> Result<Vec<u8>, String> {
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_core_graphics::{
        kCGColorSpaceSRGB, CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
    };

    let (width, height) = (CGImage::width(Some(image)), CGImage::height(Some(image)));
    let mut rgbx = vec![0u8; width * height * 4];
    let srgb = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }));
    // SAFETY: `rgbx` holds `height` rows of `width * 4` bytes and outlives
    // the context, which is dropped before it is read.
    let context = unsafe {
        CGBitmapContextCreate(
            rgbx.as_mut_ptr().cast(),
            width,
            height,
            8,
            width * 4,
            srgb.as_deref(),
            CGImageAlphaInfo::NoneSkipLast.0,
        )
    }
    .ok_or("could not draw the page's snapshot")?;
    let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width as f64, height as f64));
    CGContext::draw_image(Some(&context), rect, Some(image));
    drop(context);
    let rgb: Vec<u8> = rgbx.as_chunks::<4>().0.iter().flat_map(|&[r, g, b, _]| [r, g, b]).collect();

    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&rgb).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(png)
}

/// WebView2 encodes the PNG itself, into a memory stream, and calls back
/// on the main thread once it is written.
#[cfg(windows)]
fn capture(webview: &Webview, deadline: Instant) -> Result<Vec<u8>, String> {
    use webview2_com::CapturePreviewCompletedHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG;
    use windows::Win32::UI::Shell::SHCreateMemStream;

    let (tx, rx) = mpsc::channel::<Result<Vec<u8>, String>>();
    webview
        .with_webview(move |platform| unsafe {
            // Each failure here drops `tx`, which the wait reports.
            let Ok(core) = platform.controller().CoreWebView2() else { return };
            let Some(stream) = SHCreateMemStream(None) else { return };
            let written = stream.clone();
            let done = CapturePreviewCompletedHandler::create(Box::new(move |result| {
                let _ = tx.send(result.map_err(|e| e.to_string()).and_then(|()| read_stream(&written)));
                Ok(())
            }));
            let _ = core.CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, &stream, &done);
        })
        .map_err(|e| e.to_string())?;
    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(png) => png,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(CAPTURE_TIMED_OUT.to_string()),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err("the browser could not capture the page".to_string()),
    }
}

#[cfg(windows)]
unsafe fn read_stream(stream: &windows::Win32::System::Com::IStream) -> Result<Vec<u8>, String> {
    use windows::Win32::System::Com::STREAM_SEEK_SET;

    unsafe { stream.Seek(0, STREAM_SEEK_SET, None) }.map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let mut read = 0;
        unsafe { stream.Read(chunk.as_mut_ptr().cast(), chunk.len() as u32, Some(&mut read)) }
            .ok()
            .map_err(|e| e.to_string())?;
        if read == 0 {
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..read as usize]);
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn capture(_webview: &Webview, _deadline: Instant) -> Result<Vec<u8>, String> {
    Err("browser screenshots are not supported on this platform yet".to_string())
}

pub fn set_visible(app: &AppHandle, pane_id: &str, visible: bool) {
    // Recorded before looking for the webview: a creation that reads it
    // after this either sees it or made the webview this then finds.
    let bridge = &app.state::<AppState>().browser_bridge;
    bridge.shown.lock().unwrap().insert(pane_id.to_string(), visible);
    if let Some(webview) = app.get_webview(&label_for(pane_id)) {
        let _ = if visible {
            webview.show()
        } else {
            webview.hide()
        };
    }
}

pub fn destroy(app: &AppHandle, pane_id: &str) {
    let bridge = &app.state::<AppState>().browser_bridge;
    bridge.loaded.lock().unwrap().remove(pane_id);
    bridge.not_loaded.lock().unwrap().remove(pane_id);
    bridge.loads.lock().unwrap().remove(pane_id);
    bridge.recent.lock().unwrap().retain(|p| p != pane_id);
    bridge.shown.lock().unwrap().remove(pane_id);
    if let Some(webview) = app.get_webview(&label_for(pane_id)) {
        let _ = webview.close();
    }
}

#[cfg(test)]
mod tests {
    use super::{hidden_when_created, record_navigation};
    use cmux_core::state::PaneMeta;
    use std::collections::HashMap;
    use tauri::Url;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn a_closed_pane_is_not_brought_back() {
        let mut meta = HashMap::new();
        assert!(!record_navigation(&mut meta, "p1", &url("https://example.com/")));
        assert!(meta.is_empty());
    }

    #[test]
    fn a_pane_that_is_not_a_browser_is_left_alone() {
        let mut meta = HashMap::from([("p1".to_string(), PaneMeta::default())]);
        assert!(!record_navigation(&mut meta, "p1", &url("https://example.com/")));
        assert_eq!(meta["p1"].browser_url, None);
    }

    #[test]
    fn a_browser_pane_records_where_it_went() {
        let pane = PaneMeta { browser_url: Some("https://example.com/".into()), ..Default::default() };
        let mut meta = HashMap::from([("p1".to_string(), pane)]);
        assert!(record_navigation(&mut meta, "p1", &url("https://example.org/next")));
        assert_eq!(meta["p1"].browser_url.as_deref(), Some("https://example.org/next"));
        assert!(!record_navigation(&mut meta, "p1", &url("https://example.org/next")), "the same url again");
    }

    #[test]
    fn a_webview_starts_as_the_frontend_last_asked() {
        assert!(!hidden_when_created(Some(true), true), "shown, though on the board");
        assert!(hidden_when_created(Some(false), false), "hidden, though not on the board");
        assert!(hidden_when_created(None, true), "on the board, until its card asks");
        assert!(!hidden_when_created(None, false));
    }
}
