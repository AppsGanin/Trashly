//! Shared control for the long-running scans (Duplicates / Photos): a global
//! cancel flag plus a `scan-progress` event the UI listens to for a live count.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

static CANCEL: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Serialize)]
pub struct Progress {
    /// "walk" (discovering files) or "hash" (the slow part).
    pub phase: String,
    pub done: usize,
    /// 0 when the total isn't known yet (during the walk).
    pub total: usize,
}

/// Clear the flag at the start of a fresh scan.
pub fn reset() {
    CANCEL.store(false, Ordering::Relaxed);
}

pub fn is_cancelled() -> bool {
    CANCEL.load(Ordering::Relaxed)
}

pub fn emit(app: &AppHandle, phase: &str, done: usize, total: usize) {
    let _ = app.emit(
        "scan-progress",
        Progress {
            phase: phase.to_string(),
            done,
            total,
        },
    );
}

/// Ask the in-flight scan to stop as soon as it can.
#[tauri::command]
pub fn cancel_scan() {
    CANCEL.store(true, Ordering::Relaxed);
}
