//! Browser panes: native child webviews (Tauri multiwebview, `unstable`)
//! positioned over pane rects. Automation (snapshot/click/fill/eval) runs
//! through an injected script, whose return value Rust reads back with
//! `eval_with_callback` — remote pages get no IPC access to the app.

use std::collections::HashSet;
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
    /// Browser panes by when they were last opened or driven, latest last:
    /// the pane a command goes to when it names none.
    recent: Mutex<Vec<String>>,
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
pub fn parse_url(url: &str, current: Option<&str>) -> Result<Url, String> {
    cmux_core::browser_url::resolve(url, current)?
        .parse()
        .map_err(|e| format!("bad url {url}: {e}"))
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

/// Creates the pane's child webview if needed and applies bounds.
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

    if let Some(webview) = app.get_webview(&label) {
        let _ = webview.set_position(LogicalPosition::new(x, y));
        let _ = webview.set_size(LogicalSize::new(w.max(1.0), h.max(1.0)));
        return Ok(());
    }

    let url = {
        let state = app.state::<AppState>();
        let meta = state.meta.lock().unwrap();
        meta.get(pane_id)
            .and_then(|m| m.browser_url.clone())
            .unwrap_or_else(|| "about:blank".to_string())
    };

    let nav_pane = pane_id.to_string();
    let builder = tauri::webview::WebviewBuilder::new(&label, WebviewUrl::External(parse_url(&url, None)?))
        .initialization_script(BRIDGE_JS)
        // The page that loaded, not the one asked for: a navigation that
        // fails (nothing listening, a bad certificate) never finishes, and
        // the pane goes on showing — and reporting — the page before it.
        .on_page_load(move |webview, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let app = webview.app_handle();
                let bridge = &app.state::<AppState>().browser_bridge;
                bridge.loaded.lock().unwrap().insert(nav_pane.clone());
                bridge.not_loaded.lock().unwrap().remove(&nav_pane);
                track_navigation(app, &nav_pane, payload.url());
            }
        });

    window
        .add_child(
            builder,
            LogicalPosition::new(x, y),
            LogicalSize::new(w.max(1.0), h.max(1.0)),
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn track_navigation(app: &AppHandle, pane_id: &str, url: &Url) {
    let state = app.state::<AppState>();
    let changed = {
        let mut meta = state.meta.lock().unwrap();
        let entry = meta.entry(pane_id.to_string()).or_default();
        let new = Some(url.to_string());
        if entry.browser_url != new {
            entry.browser_url = new;
            true
        } else {
            false
        }
    };
    if changed {
        crate::commands::emit_workspace(app);
    }
}

/// The pane's webview, waiting a moment for it: the frontend creates it
/// when it first draws the pane, just after `open` has answered. Never
/// call it on the main thread, which is the one that creates it.
fn webview(app: &AppHandle, pane_id: &str) -> Result<Webview, String> {
    let is_browser = |app: &AppHandle| {
        let state = app.state::<AppState>();
        let meta = state.meta.lock().unwrap();
        meta.get(pane_id).is_some_and(|m| m.browser_url.is_some())
    };
    if !is_browser(app) {
        return Err(format!("no browser pane {pane_id}"));
    }
    let deadline = Instant::now() + WEBVIEW_WAIT;
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
    webview(app, pane_id)?;
    // A page that never loads still leaves an open pane: `open` succeeded.
    let _ = first_load(app, pane_id);
    Ok(())
}

/// Waits for the pane's first page to have loaded. Until one has, the
/// bridge is not in it — a failed load shows WebKit's error page, which
/// would leave every command to time out as if the page were busy.
fn first_load(app: &AppHandle, pane_id: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let bridge = &state.browser_bridge;
    let deadline = Instant::now() + FIRST_LOAD_WAIT;
    loop {
        if bridge.loaded.lock().unwrap().contains(pane_id) {
            return Ok(());
        }
        if bridge.not_loaded.lock().unwrap().contains(pane_id) || Instant::now() >= deadline {
            bridge.not_loaded.lock().unwrap().insert(pane_id.to_string());
            let url = state.meta.lock().unwrap().get(pane_id).and_then(|m| m.browser_url.clone());
            return Err(format!(
                "the page at {} never loaded: is anything serving it? `mira browser navigate` to another",
                url.unwrap_or_default()
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Runs an automation op inside the pane's page and gives its JSON
/// answer, waiting up to `ANSWER_WAIT` — for a Promise `eval` too.
pub fn execute(app: &AppHandle, pane_id: &str, op: serde_json::Value) -> Result<String, String> {
    execute_by(app, pane_id, op, Instant::now() + ANSWER_WAIT)
}

fn execute_by(app: &AppHandle, pane_id: &str, op: serde_json::Value, deadline: Instant) -> Result<String, String> {
    let webview = webview(app, pane_id)?;
    first_load(app, pane_id)?;
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
            match execute_by(app, pane_id, op.clone(), ask_by) {
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

pub fn navigate(app: &AppHandle, pane_id: &str, url: &str) -> Result<(), String> {
    let webview = webview(app, pane_id)?;
    // The page the pane reports, not `webview.url()`: wry unwraps the
    // webview's URL, which is nil after a first load that failed, and the
    // app panics.
    let current = {
        let state = app.state::<AppState>();
        let meta = state.meta.lock().unwrap();
        meta.get(pane_id).and_then(|m| m.browser_url.clone())
    };
    let parsed = parse_url(url, current.as_deref())?;
    webview.navigate(parsed).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn history(app: &AppHandle, pane_id: &str, action: &str) -> Result<(), String> {
    let webview = webview(app, pane_id)?;
    let js = match action {
        "back" => "history.back()",
        "forward" => "history.forward()",
        "reload" => "location.reload()",
        _ => return Err(format!("unknown history action {action}")),
    };
    webview.eval(js).map_err(|e| e.to_string())
}

pub fn set_visible(app: &AppHandle, pane_id: &str, visible: bool) {
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
    bridge.recent.lock().unwrap().retain(|p| p != pane_id);
    if let Some(webview) = app.get_webview(&label_for(pane_id)) {
        let _ = webview.close();
    }
}
