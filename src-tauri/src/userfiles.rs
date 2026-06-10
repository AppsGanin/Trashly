//! Removal of user-selected files/folders for the Duplicate Finder. Unlike the
//! cache cleaner, these touch user data — so every path is re-checked against
//! `safety::is_user_path` and defaults to the Trash (recoverable).

use std::path::PathBuf;

use serde::Serialize;

use crate::{fsutil, safety};

#[derive(Serialize)]
pub struct RemoveResult {
    pub removed: usize,
    pub freed: u64,
    /// `"<path>: <reason>"` for anything that couldn't be removed.
    pub failed: Vec<String>,
    pub needs_full_disk_access: bool,
}

/// Remove the given paths (files or folders) to the Trash or permanently. Each
/// path is re-validated, so a forged path from the UI can't escape $HOME.
#[tauri::command]
pub async fn remove_files(paths: Vec<String>, to_trash: bool) -> RemoveResult {
    tauri::async_runtime::spawn_blocking(move || remove_impl(paths, to_trash))
        .await
        .unwrap_or_else(|_| RemoveResult {
            removed: 0,
            freed: 0,
            failed: Vec::new(),
            needs_full_disk_access: false,
        })
}

fn remove_impl(paths: Vec<String>, to_trash: bool) -> RemoveResult {
    let mut removed = 0usize;
    let mut freed = 0u64;
    let mut failed = Vec::new();

    for raw in paths {
        let path = PathBuf::from(&raw);
        if !safety::is_user_path(&path) || crate::protect::is_protected(&path) {
            failed.push(format!("{}: blocked by safety guard", path.display()));
            continue;
        }
        if !path.exists() {
            continue; // already gone
        }
        let size = fsutil::dir_size(&path);
        match fsutil::delete(&path, to_trash) {
            Ok(_) => {
                removed += 1;
                freed += size;
                crate::cleanlog::record("duplicates", &raw, size, to_trash);
            }
            Err(e) => failed.push(format!("{}: {e}", path.display())),
        }
    }

    let needs_full_disk_access = failed.iter().any(|f| fsutil::is_permission_error(f));
    RemoveResult {
        removed,
        freed,
        failed,
        needs_full_disk_access,
    }
}
