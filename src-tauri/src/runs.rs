//! Command-pane run lifecycle: history records, output capture, and
//! waiters for `run --wait`. A "run" is one spawn of a command pane —
//! keypress reruns append fresh records.

use std::sync::mpsc;
use std::time::Duration;

use cmux_protocol::RunRecord;
use tauri::{AppHandle, Manager};

use crate::AppState;

const MAX_HISTORY: usize = 100;
/// Keep the tail of the output — that's where test results live.
const MAX_CAPTURE: usize = 1024 * 1024;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Called from attach_pane whenever a command pane spawns.
pub fn record_start(state: &AppState, pane_id: &str, command: &str) {
    let mut history = state.run_history.lock().unwrap();
    history.push(RunRecord {
        id: uuid::Uuid::new_v4().to_string(),
        pane_id: pane_id.to_string(),
        command: command.to_string(),
        started_ms: now_ms(),
        finished_ms: None,
        exit_code: None,
    });
    let excess = history.len().saturating_sub(MAX_HISTORY);
    if excess > 0 {
        history.drain(..excess);
    }
    state
        .run_captures
        .lock()
        .unwrap()
        .insert(pane_id.to_string(), Vec::new());
}

pub fn capture_output(state: &AppState, pane_id: &str, bytes: &[u8]) {
    let mut captures = state.run_captures.lock().unwrap();
    if let Some(buf) = captures.get_mut(pane_id) {
        buf.extend_from_slice(bytes);
        if buf.len() > MAX_CAPTURE {
            let cut = buf.len() - MAX_CAPTURE / 2;
            buf.drain(..cut);
        }
    }
}

/// Marks the open run finished; resolves waiters and fires a notification
/// so the tab lights up when an agent's command completes. Returns true
/// when the pane was a command pane.
pub fn finish_run(app: &AppHandle, pane_id: &str, exit_code: Option<i32>) -> bool {
    let state = app.state::<AppState>();
    let finished = {
        let mut history = state.run_history.lock().unwrap();
        let record = history
            .iter_mut()
            .rev()
            .find(|r| r.pane_id == pane_id && r.finished_ms.is_none());
        match record {
            Some(r) => {
                r.finished_ms = Some(now_ms());
                r.exit_code = exit_code;
                Some(r.command.clone())
            }
            None => None,
        }
    };
    let Some(command) = finished else {
        return false;
    };

    for waiter in state
        .run_waiters
        .lock()
        .unwrap()
        .remove(pane_id)
        .unwrap_or_default()
    {
        let _ = waiter.send(exit_code);
    }

    let status = match exit_code {
        Some(0) => "succeeded".to_string(),
        Some(code) => format!("failed (exit {code})"),
        None => "finished".to_string(),
    };
    crate::notify::handle_notification(
        app,
        pane_id,
        Some(format!("Command {status}")),
        command,
    );
    true
}

pub fn register_waiter(state: &AppState, pane_id: &str) -> mpsc::Receiver<Option<i32>> {
    let (tx, rx) = mpsc::channel();
    state
        .run_waiters
        .lock()
        .unwrap()
        .entry(pane_id.to_string())
        .or_default()
        .push(tx);
    rx
}

pub fn wait_for_exit(
    state: &AppState,
    rx: mpsc::Receiver<Option<i32>>,
    pane_id: &str,
    timeout_secs: u64,
) -> Result<Option<i32>, String> {
    match rx.recv_timeout(Duration::from_secs(timeout_secs)) {
        Ok(code) => Ok(code),
        Err(_) => {
            state.run_waiters.lock().unwrap().remove(pane_id);
            Err(format!(
                "run still going after {timeout_secs}s — watch it with `cmux read-screen --pane {pane_id}` or interrupt it in the pane"
            ))
        }
    }
}

/// Clean plain-text output captured for a pane's latest run.
pub fn take_capture(state: &AppState, pane_id: &str) -> String {
    let bytes = state
        .run_captures
        .lock()
        .unwrap()
        .get(pane_id)
        .cloned()
        .unwrap_or_default();
    cmux_core::osc::strip_ansi(&bytes)
}

pub fn forget_pane(state: &AppState, pane_id: &str) {
    state.run_captures.lock().unwrap().remove(pane_id);
    // A waiter on a force-closed pane resolves via finish_run when the
    // reader hits EOF; this is only the final sweep.
    state.run_waiters.lock().unwrap().remove(pane_id);
}

pub fn list(state: &AppState) -> Vec<RunRecord> {
    state.run_history.lock().unwrap().clone()
}
