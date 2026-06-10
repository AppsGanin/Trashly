//! Cleanup audit log: a small JSONL record of what each engine removed, so the
//! user can review (and find things to restore from the Trash) afterwards.

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
const MAX_ENTRIES: usize = 500;

#[derive(Serialize, Deserialize, Clone)]
pub struct LogEntry {
    /// Unix epoch seconds.
    pub time: u64,
    /// "clean" | "uninstall" | "duplicates".
    pub source: String,
    pub path: String,
    pub size: u64,
    pub to_trash: bool,
}

/// Wire up the log file path at startup.
pub fn init(app: &AppHandle) {
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = LOG_PATH.set(dir.join("cleanup-log.jsonl"));
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Append one removal record. No-op if the log path isn't set (e.g. in tests).
pub fn record(source: &str, path: &str, size: u64, to_trash: bool) {
    let Some(file) = LOG_PATH.get() else { return };
    let entry = LogEntry {
        time: now(),
        source: source.to_string(),
        path: path.to_string(),
        size,
        to_trash,
    };
    if let Ok(line) = serde_json::to_string(&entry) {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

#[tauri::command]
pub fn get_cleanup_log(limit: usize) -> Vec<LogEntry> {
    let Some(file) = LOG_PATH.get() else {
        return Vec::new();
    };
    let text = std::fs::read_to_string(file).unwrap_or_default();
    let mut entries: Vec<LogEntry> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    entries.reverse(); // newest first
    entries.truncate(limit.min(MAX_ENTRIES));
    entries
}

#[tauri::command]
pub fn clear_cleanup_log() -> Result<(), String> {
    if let Some(file) = LOG_PATH.get() {
        std::fs::write(file, "").map_err(|e| e.to_string())?;
    }
    Ok(())
}
