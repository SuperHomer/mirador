//! Hot-reload: watches mirador.json and the Ghostty config; on change,
//! re-resolves and pushes a `config-changed` event to the frontend.

use std::sync::mpsc;
use std::time::Duration;

use cmux_protocol::ResolvedConfig;
use notify::{RecursiveMode, Watcher};
use tauri::{Emitter, Manager};

use crate::AppState;

pub fn spawn(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let (tx, rx) = mpsc::channel::<()>();
        let mut watcher = match notify::recommended_watcher(
            move |res: Result<notify::Event, notify::Error>| {
                if res.is_ok() {
                    let _ = tx.send(());
                }
            },
        ) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("mirador: config watcher unavailable: {e}");
                return;
            }
        };

        // Watch directories (atomic saves replace files; watching the file
        // itself loses the handle after the first rename).
        let cfg_dir = cmux_core::config::config_dir();
        let _ = watcher.watch(&cfg_dir, RecursiveMode::NonRecursive);
        if let Some(ghostty_dir) = cmux_core::config::ghostty::user_config_path().parent() {
            if ghostty_dir.exists() {
                let _ = watcher.watch(ghostty_dir, RecursiveMode::NonRecursive);
            }
        }

        while rx.recv().is_ok() {
            // Debounce editor save bursts.
            while rx.recv_timeout(Duration::from_millis(300)).is_ok() {}
            // For the settings pane, on every change: config-changed comes
            // only when the file parses and resolves differently.
            let _ = handle.emit("config-file-changed", ());
            if let Err(err) = reload(&handle) {
                eprintln!("mirador: config error, keeping previous config: {err}");
            }
        }
    });
}

/// Makes the config files the app's config, and says so: the watcher's
/// pass after an edit, and a zoom's right after its write, whose next step
/// must start from this one (the watcher would come 300 ms later, too late
/// for a key held down). The config in force afterwards, or the file's
/// error, with the previous config kept.
///
/// One path for both, so neither misses what the other does: a changed
/// `agentsPerTab` re-cuts the wall's pages whichever of them sees it
/// first. And the file is loaded under the config lock, compared and
/// stored there, and announced before the lock goes. Loaded before it, a
/// pass that read the file just ahead of a zoom's write could store and
/// announce the older config after the zoom, until the next pass.
pub fn reload(handle: &tauri::AppHandle) -> Result<ResolvedConfig, String> {
    let state = handle.state::<AppState>();
    let (resolved, pages_changed) = {
        let mut current = state.config.lock().unwrap();
        let (cfg, err) = cmux_core::config::load();
        if let Some(err) = err {
            return Err(err);
        }
        let resolved = cmux_core::config::resolve(&cfg);
        let mut pages_changed = false;
        if *current != resolved {
            pages_changed = current.agents_per_tab != resolved.agents_per_tab;
            *current = resolved.clone();
            let _ = handle.emit("config-changed", resolved.clone());
        }
        (resolved, pages_changed)
    };
    // The wall's pages are in the snapshot, cut to agentsPerTab. After the
    // lock: building the snapshot reads the config.
    if pages_changed {
        crate::commands::broadcast_workspace(handle);
    }
    Ok(resolved)
}
