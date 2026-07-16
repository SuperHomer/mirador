//! Hot-reload: watches cmux.json and the Ghostty config; on change,
//! re-resolves and pushes a `config-changed` event to the frontend.

use std::sync::mpsc;
use std::time::Duration;

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
                eprintln!("cmux: config watcher unavailable: {e}");
                return;
            }
        };

        // Watch directories (atomic saves replace files; watching the file
        // itself loses the handle after the first rename).
        let cmux_dir = cmux_core::config::config_dir();
        let _ = watcher.watch(&cmux_dir, RecursiveMode::NonRecursive);
        if let Some(ghostty_dir) = cmux_core::config::ghostty::user_config_path().parent() {
            if ghostty_dir.exists() {
                let _ = watcher.watch(ghostty_dir, RecursiveMode::NonRecursive);
            }
        }

        while rx.recv().is_ok() {
            // Debounce editor save bursts.
            while rx.recv_timeout(Duration::from_millis(300)).is_ok() {}

            let (cfg, err) = cmux_core::config::load();
            if let Some(err) = err {
                eprintln!("cmux: config error, keeping previous config: {err}");
                continue;
            }
            let resolved = cmux_core::config::resolve(&cfg);
            let state = handle.state::<AppState>();
            let changed = {
                let mut current = state.config.lock().unwrap();
                if *current != resolved {
                    *current = resolved.clone();
                    true
                } else {
                    false
                }
            };
            if changed {
                let _ = handle.emit("config-changed", resolved);
            }
        }
    });
}
