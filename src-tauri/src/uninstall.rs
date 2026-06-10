//! App uninstaller: list installed apps, find an app's leftover files, and
//! remove the app together with the selected leftovers.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use base64::Engine;
use glob::glob;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{fsutil, safety};

#[derive(Serialize)]
pub struct AppInfo {
    pub name: String,
    pub path: String,
    pub bundle_id: String,
    pub size: u64,
    /// False for SIP-protected system apps (e.g. Safari): the bundle can't be
    /// removed, but its caches/leftovers still can.
    pub removable: bool,
}

fn app_dirs() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/Applications"),
        safety::home().join("Applications"),
    ]
}

/// True for apps that resolve into the read-only system volume (e.g. Safari is
/// a symlink to /System/Cryptexes/…). These are SIP-protected and can't be
/// uninstalled, so we hide them from the list.
fn is_system_app(p: &Path) -> bool {
    fs::canonicalize(p)
        .map(|real| real.starts_with("/System"))
        .unwrap_or(false)
}

fn bundle_id_of(app: &Path) -> String {
    let info = app.join("Contents/Info");
    let out = Command::new("/usr/bin/defaults")
        .arg("read")
        .arg(&info)
        .arg("CFBundleIdentifier")
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

#[tauri::command]
pub async fn list_apps() -> Vec<AppInfo> {
    tauri::async_runtime::spawn_blocking(list_apps_impl)
        .await
        .unwrap_or_default()
}

fn list_apps_impl() -> Vec<AppInfo> {
    // Collect bundle paths first (cheap), then size + read bundle ids in
    // parallel — each app means walking its whole tree and spawning `defaults`,
    // so doing them sequentially makes the tab feel frozen.
    let paths: Vec<PathBuf> = app_dirs()
        .iter()
        .filter_map(|dir| fs::read_dir(dir).ok())
        .flat_map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "app").unwrap_or(false))
        .collect();

    // Read bundle ids in parallel (each spawns `defaults`). Sizes are left at 0
    // and streamed in afterwards by `size_paths` so the list appears instantly.
    let mut apps: Vec<AppInfo> = paths
        .par_iter()
        .map(|p| AppInfo {
            name: p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            bundle_id: bundle_id_of(p),
            size: 0,
            removable: !is_system_app(p),
            path: p.to_string_lossy().to_string(),
        })
        .collect();

    // Don't offer to uninstall ourselves.
    apps.retain(|a| a.bundle_id != "com.ganin.trashly" && a.name != "Trashly");
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps
}

/// Whether an app currently has a running process — uninstalling a live app can
/// leave it in a weird state, so the UI warns first.
#[tauri::command]
pub fn is_app_running(app_path: String) -> bool {
    let needle = format!("{app_path}/Contents/MacOS/");
    std::process::Command::new("/usr/bin/pgrep")
        .args(["-f", &needle])
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

#[derive(Serialize)]
pub struct Leftover {
    pub path: String,
    pub label: String,
    pub size: u64,
    /// True when this path was matched by the app's *display name* rather than
    /// its bundle id. Name matches are less certain (a folder like
    /// "Application Support/Google" is shared by many apps), so the UI leaves
    /// them unchecked by default and flags them for the user to verify.
    pub from_name: bool,
}

/// Folder names under Application Support/Logs that are shared by many vendors'
/// apps — never offer these via name-based matching.
const SHARED_VENDOR_NAMES: &[&str] = &[
    "google",
    "microsoft",
    "apple",
    "adobe",
    "mozilla",
    "firefox",
    "chromium",
    "app store",
    "crashreporter",
    "caches",
];

/// A candidate leftover: (label, path, matched_by_name).
fn leftover_candidates(bundle_id: &str, name: &str) -> Vec<(String, PathBuf, bool)> {
    let h = safety::home();
    let mut out: Vec<(String, PathBuf, bool)> = Vec::new();
    let push_glob = |out: &mut Vec<(String, PathBuf, bool)>, label: &str, pat: PathBuf, v: bool| {
        if let Ok(paths) = glob(&pat.to_string_lossy()) {
            for p in paths.filter_map(|p| p.ok()) {
                out.push((label.to_string(), p, v));
            }
        }
    };

    if !bundle_id.is_empty() {
        let id = bundle_id;
        out.push(("Caches".into(), h.join("Library/Caches").join(id), false));
        out.push((
            "App Support".into(),
            h.join("Library/Application Support").join(id),
            false,
        ));
        out.push((
            "Containers".into(),
            h.join("Library/Containers").join(id),
            false,
        ));
        out.push((
            "Preferences".into(),
            h.join("Library/Preferences").join(format!("{id}.plist")),
            false,
        ));
        out.push((
            "Saved State".into(),
            h.join("Library/Saved Application State")
                .join(format!("{id}.savedState")),
            false,
        ));
        out.push((
            "Launch Agent".into(),
            h.join("Library/LaunchAgents").join(format!("{id}.plist")),
            false,
        ));
        out.push((
            "HTTP Storage".into(),
            h.join("Library/HTTPStorages").join(id),
            false,
        ));
        out.push(("WebKit".into(), h.join("Library/WebKit").join(id), false));
        out.push(("Logs".into(), h.join("Library/Logs").join(id), false));
        out.push((
            "App Scripts".into(),
            h.join("Library/Application Scripts").join(id),
            false,
        ));
        out.push((
            "Cookies".into(),
            h.join("Library/Cookies")
                .join(format!("{id}.binarycookies")),
            false,
        ));
        // By-host preferences are suffixed with a hardware UUID.
        push_glob(
            &mut out,
            "Preferences",
            h.join("Library/Preferences/ByHost").join(format!("{id}.*")),
            false,
        );
        // Group containers are suffixed/prefixed; match by glob.
        push_glob(
            &mut out,
            "Group Container",
            h.join("Library/Group Containers").join(format!("*{id}*")),
            false,
        );
        // Heavy app-specific data not keyed by bundle id (often the real space).
        // The bool marks shared/uncertain dirs that must NOT be auto-selected.
        for (label, p, verify) in app_specific_extras(id, &h) {
            out.push((label, p, verify));
        }
    }

    // Name-based matches, but only for names that aren't shared vendor folders.
    if !name.is_empty() && !SHARED_VENDOR_NAMES.contains(&name.to_lowercase().as_str()) {
        out.push((
            "App Support".into(),
            h.join("Library/Application Support").join(name),
            true,
        ));
        out.push(("Logs".into(), h.join("Library/Logs").join(name), true));
        // Crash reports are named after the executable (verify — fuzzy match).
        push_glob(
            &mut out,
            "Crash Reports",
            h.join("Library/Logs/DiagnosticReports")
                .join(format!("{name}*")),
            true,
        );
    }
    out
}

/// Known heavy data directories per app that aren't keyed by bundle id — for
/// dev tools this is where the real tens-of-GB live (Xcode DerivedData/device
/// support, Android SDK/IDE caches), which the generic bundle-id lookup misses.
/// The bool flags shared/uncertain dirs (e.g. the Android SDK) so the UI marks
/// them "verify" and leaves them unchecked.
fn app_specific_extras(bundle_id: &str, h: &Path) -> Vec<(String, PathBuf, bool)> {
    let mut v: Vec<(String, PathBuf, bool)> = Vec::new();
    // Expand a glob pattern (under $HOME) and push each match.
    let mut push_glob = |label: &str, pat: PathBuf, verify: bool| {
        if let Ok(paths) = glob(&pat.to_string_lossy()) {
            for p in paths.filter_map(|p| p.ok()) {
                v.push((label.to_string(), p, verify));
            }
        }
    };

    // JetBrains IDEs keep their (huge) caches/indexes/settings under
    // ~/Library/*/JetBrains/<Product><version>, not under the bundle id.
    let jb_product = match bundle_id {
        "com.jetbrains.intellij" => Some("IntelliJIdea"),
        "com.jetbrains.intellij.ce" => Some("IdeaIC"),
        "com.jetbrains.pycharm" => Some("PyCharm"),
        "com.jetbrains.pycharm.ce" => Some("PyCharmCE"),
        "com.jetbrains.WebStorm" => Some("WebStorm"),
        "com.jetbrains.goland" => Some("GoLand"),
        "com.jetbrains.CLion" => Some("CLion"),
        "com.jetbrains.PhpStorm" => Some("PhpStorm"),
        "com.jetbrains.rubymine" => Some("RubyMine"),
        "com.jetbrains.rider" => Some("Rider"),
        "com.jetbrains.datagrip" => Some("DataGrip"),
        "com.jetbrains.dataspell" => Some("DataSpell"),
        "com.jetbrains.rustrover" => Some("RustRover"),
        _ => None,
    };
    if let Some(prod) = jb_product {
        push_glob(
            "IDE Caches",
            h.join(format!("Library/Caches/JetBrains/{prod}*")),
            false,
        );
        push_glob(
            "IDE Settings",
            h.join(format!("Library/Application Support/JetBrains/{prod}*")),
            true, // contains user settings/plugins
        );
        push_glob(
            "IDE Logs",
            h.join(format!("Library/Logs/JetBrains/{prod}*")),
            false,
        );
    }

    match bundle_id {
        // ── IDEs / editors ──────────────────────────────────────────
        "com.apple.dt.Xcode" => {
            let dev = h.join("Library/Developer");
            v.push(("Derived Data".into(), dev.join("Xcode/DerivedData"), false));
            v.push((
                "iOS DeviceSupport".into(),
                dev.join("Xcode/iOS DeviceSupport"),
                false,
            ));
            v.push((
                "watchOS DeviceSupport".into(),
                dev.join("Xcode/watchOS DeviceSupport"),
                false,
            ));
            v.push((
                "tvOS DeviceSupport".into(),
                dev.join("Xcode/tvOS DeviceSupport"),
                false,
            ));
            v.push((
                "Simulator Caches".into(),
                dev.join("CoreSimulator/Caches"),
                false,
            ));
        }
        "com.google.android.studio" => {
            push_glob(
                "IDE Caches",
                h.join("Library/Caches/Google/AndroidStudio*"),
                false,
            );
            push_glob(
                "IDE Settings",
                h.join("Library/Application Support/Google/AndroidStudio*"),
                true,
            );
            push_glob(
                "IDE Logs",
                h.join("Library/Logs/Google/AndroidStudio*"),
                false,
            );
            v.push(("Android SDK".into(), h.join("Library/Android/sdk"), true));
            v.push(("Emulators (.android)".into(), h.join(".android"), true));
            v.push(("Gradle cache".into(), h.join(".gradle"), true));
        }
        "com.microsoft.VSCode" => {
            v.push((
                "App Data".into(),
                h.join("Library/Application Support/Code"),
                true,
            ));
            v.push(("Extensions".into(), h.join(".vscode"), true));
        }
        "com.todesktop.230313mzl4w4u92" | "com.cursor.Cursor" => {
            v.push((
                "App Data".into(),
                h.join("Library/Application Support/Cursor"),
                true,
            ));
            v.push(("Extensions".into(), h.join(".cursor"), true));
        }
        "com.sublimetext.4" | "com.sublimetext.3" => {
            v.push((
                "Packages".into(),
                h.join("Library/Application Support/Sublime Text"),
                true,
            ));
        }

        // ── Containers / VMs / k8s ──────────────────────────────────
        "com.docker.docker" => {
            // The big VM disk lives in the bundle-id Container (already found).
            v.push(("Docker config".into(), h.join(".docker"), true));
        }
        "dev.orbstack.OrbStack" => {
            v.push(("OrbStack data".into(), h.join(".orbstack"), true));
        }
        "io.rancherdesktop.app" => {
            v.push((
                "Rancher data".into(),
                h.join("Library/Application Support/rancher-desktop"),
                true,
            ));
            v.push((".rd".into(), h.join(".rd"), true));
        }
        "com.electron.lens" | "com.k8slens.desktop" => {
            v.push((
                "Lens data".into(),
                h.join("Library/Application Support/Lens"),
                true,
            ));
            v.push(("Kube cache".into(), h.join(".kube/cache"), false));
        }

        // ── Browsers (profiles are user data → verify) ──────────────
        "com.google.Chrome" => {
            v.push((
                "Profile".into(),
                h.join("Library/Application Support/Google/Chrome"),
                true,
            ));
            v.push((
                "Cache".into(),
                h.join("Library/Caches/Google/Chrome"),
                false,
            ));
        }
        "org.mozilla.firefox" => {
            v.push((
                "Profiles".into(),
                h.join("Library/Application Support/Firefox"),
                true,
            ));
            v.push(("Cache".into(), h.join("Library/Caches/Firefox"), false));
        }
        "com.microsoft.edgemac" => {
            v.push((
                "Profile".into(),
                h.join("Library/Application Support/Microsoft Edge"),
                true,
            ));
            v.push((
                "Cache".into(),
                h.join("Library/Caches/Microsoft Edge"),
                false,
            ));
        }
        "com.brave.Browser" => {
            v.push((
                "Profile".into(),
                h.join("Library/Application Support/BraveSoftware"),
                true,
            ));
            v.push((
                "Cache".into(),
                h.join("Library/Caches/BraveSoftware"),
                false,
            ));
        }

        // ── Messaging / social (Electron caches under app name) ─────
        "com.hnc.Discord" => {
            v.push((
                "App Data".into(),
                h.join("Library/Application Support/discord"),
                false,
            ));
        }
        "com.tinyspeck.slackmacgap" => {
            v.push((
                "App Data".into(),
                h.join("Library/Application Support/Slack"),
                false,
            ));
        }

        _ => {}
    }
    v
}

#[tauri::command]
pub async fn app_leftovers(bundle_id: String, name: String) -> Vec<Leftover> {
    tauri::async_runtime::spawn_blocking(move || app_leftovers_impl(bundle_id, name))
        .await
        .unwrap_or_default()
}

fn app_leftovers_impl(bundle_id: String, name: String) -> Vec<Leftover> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for (label, path, from_name) in leftover_candidates(&bundle_id, &name) {
        if !path.exists() || !safety::is_uninstall_target(&path) {
            continue;
        }
        let key = path.to_string_lossy().to_string();
        if !seen.insert(key.clone()) {
            continue;
        }
        result.push(Leftover {
            path: key,
            label,
            size: fsutil::dir_size(&path),
            from_name,
        });
    }
    result.sort_by(|a, b| b.size.cmp(&a.size));
    result
}

#[derive(Deserialize)]
pub struct UninstallRequest {
    pub app_path: String,
    pub leftover_paths: Vec<String>,
    pub to_trash: bool,
    /// False for system apps whose bundle can't be removed — only the leftovers.
    pub remove_bundle: bool,
}

#[derive(Serialize)]
pub struct UninstallResult {
    pub removed: usize,
    pub freed: u64,
    /// Each entry is `"<path>: <reason>"` so the UI can show why it failed.
    pub failed: Vec<String>,
    /// True when a failure looked like a permission block (needs Full Disk Access).
    pub needs_full_disk_access: bool,
}

#[tauri::command]
pub async fn uninstall(req: UninstallRequest) -> UninstallResult {
    tauri::async_runtime::spawn_blocking(move || uninstall_impl(req))
        .await
        .unwrap_or_else(|_| UninstallResult {
            removed: 0,
            freed: 0,
            failed: Vec::new(),
            needs_full_disk_access: false,
        })
}

fn uninstall_impl(req: UninstallRequest) -> UninstallResult {
    let mut targets: Vec<PathBuf> = Vec::new();
    if req.remove_bundle {
        targets.push(PathBuf::from(&req.app_path));
    }
    targets.extend(req.leftover_paths.iter().map(PathBuf::from));

    let mut removed = 0usize;
    let mut freed = 0u64;
    let mut failed = Vec::new();

    for path in targets {
        if !safety::is_uninstall_target(&path) || crate::protect::is_protected(&path) {
            failed.push(format!("{}: blocked by safety guard", path.display()));
            continue;
        }
        if !path.exists() {
            continue;
        }
        let size = fsutil::dir_size(&path);
        match fsutil::delete(&path, req.to_trash) {
            Ok(_) => {
                removed += 1;
                freed += size;
                crate::cleanlog::record("uninstall", &path.to_string_lossy(), size, req.to_trash);
            }
            Err(e) => failed.push(format!("{}: {e}", path.display())),
        }
    }

    let needs_full_disk_access = failed.iter().any(|f| fsutil::is_permission_error(f));
    UninstallResult {
        removed,
        freed,
        failed,
        needs_full_disk_access,
    }
}

/// Returns a PNG data-URL of an app's icon, or `None` if it can't be produced.
/// The bundle's .icns is converted to a 64px PNG via the system `sips` tool.
#[tauri::command]
pub async fn app_icon(app_path: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || app_icon_impl(app_path))
        .await
        .unwrap_or(None)
}

fn app_icon_impl(app_path: String) -> Option<String> {
    let app = PathBuf::from(&app_path);
    if app.extension().map(|e| e != "app").unwrap_or(true) {
        return None;
    }

    // CFBundleIconFile names the icon resource (with or without .icns).
    let info = app.join("Contents/Info");
    let out = Command::new("/usr/bin/defaults")
        .arg("read")
        .arg(&info)
        .arg("CFBundleIconFile")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let mut icon_name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if icon_name.is_empty() {
        return None;
    }
    if !icon_name.ends_with(".icns") {
        icon_name.push_str(".icns");
    }
    let icns = app.join("Contents/Resources").join(&icon_name);
    if !icns.exists() {
        return None;
    }

    // Convert to a small PNG in a temp file, then read it back.
    let stem: String = app_path
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let png = std::env::temp_dir().join(format!("trashly-icon-{stem}.png"));
    let status = Command::new("/usr/bin/sips")
        .args(["-z", "64", "64", "-s", "format", "png"])
        .arg(&icns)
        .arg("--out")
        .arg(&png)
        .output()
        .ok()?;
    if !status.status.success() {
        return None;
    }
    let bytes = fs::read(&png).ok()?;
    let _ = fs::remove_file(&png);
    let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
    Some(format!("data:image/png;base64,{b64}"))
}
