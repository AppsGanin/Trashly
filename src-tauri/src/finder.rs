//! Small Finder/Quick Look helpers so the user can inspect an item before
//! deleting it — building trust in what the cleaners are about to remove.

use std::process::{Command, Stdio};

/// Reveal a path in Finder (selects it in its containing folder).
#[tauri::command]
pub fn reveal_in_finder(path: String) {
    let _ = Command::new("/usr/bin/open").args(["-R", &path]).spawn();
}

/// Open a macOS Quick Look preview of a path.
#[tauri::command]
pub fn quick_look(path: String) {
    let _ = Command::new("/usr/bin/qlmanage")
        .args(["-p", &path])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Whether Trashly has Full Disk Access — probed by reading the user's TCC
/// database, which only opens with FDA. When granted, scanning Desktop /
/// Documents / Downloads no longer triggers per-folder permission prompts.
#[tauri::command]
pub fn has_full_disk_access() -> bool {
    let tcc = crate::safety::home().join("Library/Application Support/com.apple.TCC/TCC.db");
    std::fs::File::open(tcc).is_ok()
}
