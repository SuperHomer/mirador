//! Browser panes: native child webviews (Tauri multiwebview, `unstable`)
//! positioned over pane rects. Automation (snapshot/click/fill/eval) runs
//! through an injected script; results come back by navigating to a
//! `cmux-result://` URL we intercept and cancel — remote pages get no IPC
//! access to the app.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl};

use crate::AppState;

const BRIDGE_JS: &str = include_str!("browser_bridge.js");

#[derive(Default)]
pub struct BrowserBridge {
    pending: Mutex<HashMap<u64, mpsc::Sender<String>>>,
    next_id: AtomicU64,
}

fn label_for(pane_id: &str) -> String {
    let safe: String = pane_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    format!("browser-{safe}")
}

fn parse_url(url: &str) -> Result<Url, String> {
    let candidate = if url.contains("://") || url == "about:blank" {
        url.to_string()
    } else {
        format!("https://{url}")
    };
    candidate.parse().map_err(|e| format!("bad url: {e}"))
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

    let window = app
        .get_window("main")
        .ok_or_else(|| "main window missing".to_string())?;

    let nav_app = app.clone();
    let nav_pane = pane_id.to_string();
    let builder = tauri::webview::WebviewBuilder::new(&label, WebviewUrl::External(parse_url(&url)?))
        .initialization_script(BRIDGE_JS)
        .on_navigation(move |url| {
            if url.scheme() == "cmux-result" {
                handle_result(&nav_app, url);
                return false;
            }
            track_navigation(&nav_app, &nav_pane, url);
            true
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

/// `cmux-result://r/<request_id>/<base64url-json>`
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

/// Runs an automation op inside the pane's webview, waiting for the
/// bridge result (10s timeout).
pub fn execute(app: &AppHandle, pane_id: &str, op: serde_json::Value) -> Result<String, String> {
    let label = label_for(pane_id);
    let webview = app
        .get_webview(&label)
        .ok_or_else(|| format!("no browser pane {pane_id}"))?;

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
        .eval(format!("window.__cmuxRun({request_id}, {op_json})"))
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
    let parsed = parse_url(url)?;
    let webview = app
        .get_webview(&label_for(pane_id))
        .ok_or_else(|| format!("no browser pane {pane_id}"))?;
    webview.navigate(parsed).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn history(app: &AppHandle, pane_id: &str, action: &str) -> Result<(), String> {
    let webview = app
        .get_webview(&label_for(pane_id))
        .ok_or_else(|| format!("no browser pane {pane_id}"))?;
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
    if let Some(webview) = app.get_webview(&label_for(pane_id)) {
        let _ = webview.close();
    }
}
