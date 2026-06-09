//! Shared filesystem helpers used by both the clean and uninstall engines.

use std::fs;
use std::path::Path;

use walkdir::WalkDir;

/// Recursively sum the on-disk size of a path. A symlinked *root* is resolved
/// to its target first (e.g. /Applications/Safari.app → System/Cryptexes/…),
/// otherwise we'd report the bytes of the link instead of the real bundle.
/// Symlinks *inside* the tree are not followed.
pub fn dir_size(path: &Path) -> u64 {
    let resolved = if path.is_symlink() {
        fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    } else {
        path.to_path_buf()
    };
    let path = resolved.as_path();
    if path.is_file() {
        return fs::symlink_metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// Move a path to the macOS Trash via `NSFileManager.trashItemAtURL` — the
/// proper Finder Trash (restorable with "Put Back"). We pick this over the
/// crate's default Finder/AppleScript method so it needs no Automation
/// permission prompt and handles large directories reliably.
pub fn trash_to_bin(path: &Path) -> Result<(), String> {
    use trash::macos::{DeleteMethod, TrashContextExtMacos};
    let mut ctx = trash::TrashContext::default();
    ctx.set_delete_method(DeleteMethod::NsFileManager);
    ctx.delete(path).map_err(|e| e.to_string())
}

/// Permanently remove a path (file, symlink, or directory tree).
fn remove(path: &Path) -> std::io::Result<()> {
    if path.is_dir() && !path.is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Delete a path either to the Trash (recoverable) or permanently.
pub fn delete(path: &Path, to_trash: bool) -> Result<(), String> {
    if to_trash {
        trash_to_bin(path)
    } else {
        remove(path).map_err(|e| e.to_string())
    }
}

/// Heuristic: does this error message indicate the OS blocked us for lack of
/// permission? On macOS that usually means Trashly needs Full Disk Access.
pub fn is_permission_error(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("operation not permitted")
        || m.contains("permission denied")
        || m.contains("os error 1)")
        || m.contains("os error 13")
}
