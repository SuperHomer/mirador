//! Notification handling: stores notifications from the OSC scanner, marks
//! them read on focus, decorates workspace snapshots with unread info, and
//! raises native OS notifications while the window is unfocused.

use std::sync::atomic::Ordering;

use cmux_protocol::NotificationDto;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::{NotificationExt, PermissionState};

use crate::commands::emit_workspace;
use crate::AppState;

const MAX_NOTIFICATIONS: usize = 200;

/// Requests OS notification permission on launch so that unfocused
/// notifications can actually be delivered. macOS in particular stays
/// silent until the app has been explicitly authorized.
pub fn ensure_permission(app: &AppHandle) {
    let notifier = app.notification();
    let state = match notifier.permission_state() {
        Ok(state) => state,
        Err(e) => {
            eprintln!("mirador: could not read notification permission: {e}");
            return;
        }
    };
    if matches!(state, PermissionState::Granted) {
        return;
    }
    match notifier.request_permission() {
        Ok(PermissionState::Granted) => {}
        Ok(other) => eprintln!(
            "mirador: notifications not permitted ({other:?}); enable them in \
             System Settings › Notifications › Mirador"
        ),
        Err(e) => eprintln!("mirador: notification permission request failed: {e}"),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Entry point for OSC notification events (runs on a forwarder thread).
pub fn handle_notification(
    app: &AppHandle,
    pane_id: &str,
    title: Option<String>,
    body: String,
) {
    let state = app.state::<AppState>();
    let window_focused = state.window_focused.load(Ordering::Relaxed);

    // A notification for the pane you're looking at needs no attention ring.
    let visible = window_focused && {
        let ws = state.workspace.lock().unwrap();
        let tab = ws.active_tab();
        tab.focused == pane_id
    };

    let dto = NotificationDto {
        id: uuid::Uuid::new_v4().to_string(),
        pane_id: pane_id.to_string(),
        title,
        body,
        at_ms: now_ms(),
        read: visible,
    };

    {
        let mut list = state.notifications.lock().unwrap();
        list.push(dto.clone());
        let excess = list.len().saturating_sub(MAX_NOTIFICATIONS);
        if excess > 0 {
            list.drain(..excess);
        }
    }

    let _ = app.emit("notification", &dto);
    emit_workspace(app);

    if !window_focused {
        let title = dto.title.as_deref().unwrap_or("Mirador");
        if let Err(e) = app
            .notification()
            .builder()
            .title(title)
            .body(&dto.body)
            .show()
        {
            eprintln!(
                "mirador: failed to show notification: {e}; check System Settings › \
                 Notifications › Mirador"
            );
        }
    }
}

/// Marks a pane's notifications read (called when it gains focus).
pub fn mark_pane_read(state: &AppState, pane_id: &str) -> bool {
    let mut changed = false;
    for n in state.notifications.lock().unwrap().iter_mut() {
        if n.pane_id == pane_id && !n.read {
            n.read = true;
            changed = true;
        }
    }
    changed
}

pub fn mark_all_read(state: &AppState) -> bool {
    let mut changed = false;
    for n in state.notifications.lock().unwrap().iter_mut() {
        if !n.read {
            n.read = true;
            changed = true;
        }
    }
    changed
}

/// Drops notifications belonging to killed panes.
pub fn forget_panes(state: &AppState, panes: &[String]) {
    state
        .notifications
        .lock()
        .unwrap()
        .retain(|n| !panes.contains(&n.pane_id));
}

/// Fills the unread/last-notification fields of a fresh snapshot.
pub fn decorate_snapshot(
    state: &AppState,
    snapshot: &mut cmux_protocol::WorkspaceSnapshot,
) {
    let list = state.notifications.lock().unwrap();
    let unread: Vec<&NotificationDto> = list.iter().filter(|n| !n.read).collect();

    snapshot.unread_panes = unread.iter().map(|n| n.pane_id.clone()).collect();
    snapshot.unread_panes.dedup();

    for tab in &mut snapshot.tabs {
        let panes = cmux_core::layout::pane_ids(&tab.root);
        tab.unread = unread
            .iter()
            .filter(|n| panes.contains(&n.pane_id))
            .count() as u32;
        tab.last_notification = list
            .iter()
            .rev()
            .find(|n| panes.contains(&n.pane_id))
            .map(|n| n.body.clone());
    }
}
