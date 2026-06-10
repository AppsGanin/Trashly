//! User-defined protected folders. Anything inside a protected path is refused
//! by every deletion engine (Clean, Uninstall, Duplicates) — a hard override on
//! top of the built-in safety guards.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use tauri::{AppHandle, Manager};

static PROTECTED: RwLock<Vec<PathBuf>> = RwLock::new(Vec::new());

fn store_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("protected-paths.json"))
}

/// Load the saved list into memory at startup.
pub fn load(app: &AppHandle) {
    let list: Vec<PathBuf> = store_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .map(|v| v.into_iter().map(PathBuf::from).collect())
        .unwrap_or_default();
    if let Ok(mut g) = PROTECTED.write() {
        *g = list;
    }
}

/// True when `target` is, or lives under, a user-protected folder.
pub fn is_protected(target: &Path) -> bool {
    PROTECTED
        .read()
        .map(|g| g.iter().any(|p| target == p || target.starts_with(p)))
        .unwrap_or(false)
}

#[tauri::command]
pub fn get_protected_paths() -> Vec<String> {
    PROTECTED
        .read()
        .map(|g| g.iter().map(|p| p.to_string_lossy().to_string()).collect())
        .unwrap_or_default()
}

#[tauri::command]
pub fn set_protected_paths(app: AppHandle, paths: Vec<String>) -> Result<(), String> {
    // Keep only absolute, de-duplicated paths.
    let mut clean: Vec<PathBuf> = Vec::new();
    for p in paths {
        let pb = PathBuf::from(&p);
        if pb.is_absolute() && !clean.contains(&pb) {
            clean.push(pb);
        }
    }
    if let Some(file) = store_path(&app) {
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let as_str: Vec<String> = clean
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        if let Ok(json) = serde_json::to_string_pretty(&as_str) {
            std::fs::write(&file, json).map_err(|e| e.to_string())?;
        }
    }
    if let Ok(mut g) = PROTECTED.write() {
        *g = clean;
    }
    Ok(())
}
