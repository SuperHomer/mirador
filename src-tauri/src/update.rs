//! Update checking and installation (tauri-plugin-updater).
//!
//! The app asks GitHub's `releases/latest` for a signed `latest.json`,
//! compares its version against this build's, and tells the user. Installing
//! is always the user's choice — nothing downloads or replaces the app until
//! they ask, because an update that restarts the terminal underneath a
//! running agent is not a favour.
//!
//! Payloads are verified against the public key baked into tauri.conf.json,
//! so a tampered release cannot install even though the app itself carries no
//! Apple signature.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::UpdaterExt;

use crate::AppState;

/// What the frontend needs to offer an update, and nothing more.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// Version on offer, e.g. "0.1.13".
    pub version: String,
    /// Version running now, so the UI can say "0.1.12 → 0.1.13".
    pub current_version: String,
    /// Release notes, when the manifest carries them.
    pub notes: Option<String>,
}

#[derive(Default)]
pub struct UpdateState {
    /// Set by whichever check found an update; read by the frontend on mount.
    pub available: Mutex<Option<UpdateInfo>>,
}

/// Seconds to wait before the first check. Launch is the busiest moment of
/// the app's life — session restore, pane spawning, the PATH lookup — and an
/// update is never urgent enough to compete with it.
const STARTUP_DELAY_SECS: u64 = 10;

/// Checks shortly after launch, then once a day for sessions left running.
/// Failure is silence: no network, a rate limit, or a malformed manifest
/// should never produce a dialog or a log the user has to care about.
pub fn spawn_check_loop(app: AppHandle) {
    // A plain thread blocking on the check, as the intel poller does — the
    // alternative is a tokio timer dependency for two sleeps.
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(STARTUP_DELAY_SECS));
        loop {
            if let Ok(Some(info)) = tauri::async_runtime::block_on(look(&app)) {
                announce(&app, info);
            }
            std::thread::sleep(std::time::Duration::from_secs(60 * 60 * 24));
        }
    });
}

/// Asks the endpoint what it has. `Ok(None)` means up to date.
async fn look(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let found = updater.check().await.map_err(|e| e.to_string())?;
    Ok(found.map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        notes: u.body.clone(),
    }))
}

/// Records the update and tells the window. The event can outrun the
/// frontend's listener on a cold start, so `available_update` is the
/// authority and the event is only a nudge.
fn announce(app: &AppHandle, info: UpdateInfo) {
    {
        let state = app.state::<AppState>();
        let mut slot = state.update.available.lock().unwrap();
        if slot.as_ref().map(|i| i.version.clone()) == Some(info.version.clone()) {
            return; // already announced this one
        }
        *slot = Some(info.clone());
    }
    let _ = app.emit("update-available", info.clone());
    // A native notification is the only thing that reaches a user who is not
    // looking at the window, which at 10 seconds after launch is most of them.
    let _ = app
        .notification()
        .builder()
        .title("Mirador update available")
        .body(format!(
            "{} is out — you have {}.",
            info.version, info.current_version
        ))
        .show();
}

/// The update found by a previous check, if any. The frontend calls this on
/// mount so a check that finished first is not lost.
#[tauri::command]
pub fn available_update(state: tauri::State<'_, AppState>) -> Option<UpdateInfo> {
    state.update.available.lock().unwrap().clone()
}

/// Checks on demand — the command palette's "Check for Updates".
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let found = look(&app).await?;
    if let Some(info) = found.clone() {
        announce(&app, info);
    }
    Ok(found)
}

/// Downloads, verifies and installs, then restarts. Only ever called because
/// the user asked: `restart` takes the window down with it.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no update available".to_string())?;

    let progress = app.clone();
    let mut downloaded = 0usize;
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk;
                let _ = progress.emit(
                    "update-progress",
                    serde_json::json!({ "downloaded": downloaded, "total": total }),
                );
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;

    // Scrollback and layout are written on exit; give that its chance before
    // the process goes away under the new bundle.
    crate::save_session(&app.state::<AppState>());
    app.restart();
}
