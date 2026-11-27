//! Markdown panes follow their file: the parent directory of each is
//! watched (`cmux_core::follow` says which, and why the directory), and a
//! change emits `markdown-changed` with the path, once a save's burst of
//! events has settled.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use cmux_core::follow::Follows;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::AppState;

/// One save is often several events (write, rename, chmod); the pane reads
/// the file once they stop.
const SETTLE: Duration = Duration::from_millis(150);

#[derive(Default)]
pub struct MarkdownWatch {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    follows: Follows,
    /// Made on the first follow, with the thread that reads its events.
    watcher: Option<RecommendedWatcher>,
}

/// The pane's file is followed while the pane is shown. The path is the
/// pane's own, from its meta: the page names a pane, never a path.
#[tauri::command]
pub fn watch_markdown(app: AppHandle, state: State<'_, AppState>, pane_id: String) -> Result<(), String> {
    let path = {
        let meta = state.meta.lock().unwrap();
        let m = meta.get(&pane_id).ok_or_else(|| format!("no pane {pane_id}"))?;
        m.markdown_path.clone().ok_or("not a markdown pane")?
    };
    let mut inner = state.markdown_watch.inner.lock().unwrap();
    if inner.watcher.is_none() {
        inner.watcher = Some(spawn(app.clone())?);
    }
    let (start, stop) = inner.follows.follow(&pane_id, &path);
    let watcher = inner.watcher.as_mut().expect("made above");
    if let Some(dir) = stop {
        let _ = watcher.unwatch(&dir);
    }
    if let Some(dir) = start {
        if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
            inner.follows.unfollow(&pane_id);
            return Err(format!("cannot follow {path}: {e}"));
        }
    }
    Ok(())
}

#[tauri::command]
pub fn unwatch_markdown(state: State<'_, AppState>, pane_id: String) {
    let mut inner = state.markdown_watch.inner.lock().unwrap();
    if let Some(dir) = inner.follows.unfollow(&pane_id) {
        if let Some(watcher) = inner.watcher.as_mut() {
            let _ = watcher.unwatch(&dir);
        }
    }
}

fn spawn(app: AppHandle) -> Result<RecommendedWatcher, String> {
    let (tx, rx) = mpsc::channel::<std::path::PathBuf>();
    let watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
        let Ok(event) = res else { return };
        // Reading the file is an access event on Linux: answering it with
        // a read would loop for ever.
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        for path in event.paths {
            let _ = tx.send(path);
        }
    })
    .map_err(|e| format!("file watcher unavailable: {e}"))?;

    std::thread::spawn(move || {
        // Each changed path, as a pane spells it, and when it last changed.
        let mut pending: HashMap<String, Instant> = HashMap::new();
        loop {
            let event = match pending.values().min() {
                Some(&earliest) => rx.recv_timeout((earliest + SETTLE).saturating_duration_since(Instant::now())),
                None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
            };
            match event {
                Ok(path) => {
                    let changed = app.state::<AppState>().markdown_watch.inner.lock().unwrap().follows.changed(&path);
                    for p in changed {
                        pending.insert(p, Instant::now());
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
            let now = Instant::now();
            pending.retain(|path, at| {
                let settled = now.duration_since(*at) >= SETTLE;
                if settled {
                    let _ = app.emit("markdown-changed", path.clone());
                }
                !settled
            });
        }
    });
    Ok(watcher)
}
