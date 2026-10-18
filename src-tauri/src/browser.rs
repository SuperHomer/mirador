//! Browser panes: native child webviews (Tauri multiwebview, `unstable`)
//! positioned over pane rects. Automation (snapshot/click/fill/eval) runs
//! through an injected script; results come back by navigating to a
//! `mira-result://` URL we intercept and cancel — remote pages get no IPC
//! access to the app.

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
    pending: Mutex<HashMap<u64, mpsc::Sender<String>>>,
    next_id: AtomicU64,
    /// Memoized host-view origin. Cleared by `forget_host_origin` when the
    /// window moves or resizes, which is the only time it can change.
    host_origin: Mutex<Option<(f64, f64)>>,
    /// Panes whose webview has finished loading a page at least once.
    loaded: Mutex<HashSet<String>>,
}

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

    let nav_app = app.clone();
    let nav_pane = pane_id.to_string();
    let builder = tauri::webview::WebviewBuilder::new(&label, WebviewUrl::External(parse_url(&url, None)?))
        .initialization_script(BRIDGE_JS)
        .on_navigation(move |url| {
            if url.scheme() == "mira-result" {
                handle_result(&nav_app, url);
                return false;
            }
            true
        })
        // The page that loaded, not the one asked for: a navigation that
        // fails (nothing listening, a bad certificate) never finishes, and
        // the pane goes on showing — and reporting — the page before it.
        .on_page_load(move |webview, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let app = webview.app_handle();
                app.state::<AppState>().browser_bridge.loaded.lock().unwrap().insert(nav_pane.clone());
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

/// `mira-result://r/<request_id>/<base64url-json>`
fn handle_result(app: &AppHandle, url: &Url) {
    let path = url.path().trim_start_matches('/');
    let mut parts = path.splitn(2, '/');
    let Some(id) = parts.next().and_then(|s| s.parse::<u64>().ok()) else {
        return;
    };
    let payload = parts.next().unwrap_or("");
    let json = decode_base64url(payload).unwrap_or_else(|| "{\"error\":\"bad payload\"}".into());
    let state = app.state::<AppState>();
    let tx = state.browser_bridge.pending.lock().unwrap().remove(&id);
    if let Some(tx) = tx {
        let _ = tx.send(json);
    }
}

fn decode_base64url(s: &str) -> Option<String> {
    let standard: String = s
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            other => other,
        })
        .collect();
    cmux_core::osc::decode_base64(&standard)
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
    let deadline = Instant::now() + FIRST_LOAD_WAIT;
    let state = app.state::<AppState>();
    while Instant::now() < deadline && !state.browser_bridge.loaded.lock().unwrap().contains(pane_id) {
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

/// Runs an automation op inside the pane's webview, waiting for the
/// bridge result (10s timeout).
pub fn execute(app: &AppHandle, pane_id: &str, op: serde_json::Value) -> Result<String, String> {
    let webview = webview(app, pane_id)?;

    let state = app.state::<AppState>();
    let request_id = state.browser_bridge.next_id.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::channel();
    state
        .browser_bridge
        .pending
        .lock()
        .unwrap()
        .insert(request_id, tx);

    let op_json = serde_json::to_string(&op).map_err(|e| e.to_string())?;
    webview
        .eval(format!("window.__miraRun({request_id}, {op_json})"))
        .map_err(|e| e.to_string())?;

    let result = rx
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "browser automation timed out (page busy or bridge blocked?)".to_string());
    state
        .browser_bridge
        .pending
        .lock()
        .unwrap()
        .remove(&request_id);
    result
}

pub fn navigate(app: &AppHandle, pane_id: &str, url: &str) -> Result<(), String> {
    let webview = webview(app, pane_id)?;
    let current = webview.url().ok().map(String::from);
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
    app.state::<AppState>().browser_bridge.loaded.lock().unwrap().remove(pane_id);
    if let Some(webview) = app.get_webview(&label_for(pane_id)) {
        let _ = webview.close();
    }
}
