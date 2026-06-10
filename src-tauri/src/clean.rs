//! Clean engine: scan categories of reclaimable space, then delete selected
//! entries either to the Trash or directly.

use std::fs;
use std::path::{Path, PathBuf};

use glob::glob;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::{fsutil, safety};

/// How a category's targets are discovered.
enum Source {
    /// Each immediate child of `dir` is a separate deletable entry.
    /// Lets the user keep some app caches and clear others.
    DirChildren(PathBuf),
    /// `path` itself is a single deletable entry.
    Single(PathBuf),
    /// Glob pattern; each match is a deletable entry.
    Glob(String),
}

struct CategoryDef {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    /// "safe" or "caution"
    risk: &'static str,
    /// When true, entries are always removed directly (Trash makes no sense,
    /// e.g. emptying the Trash itself).
    always_direct: bool,
    sources: Vec<Source>,
}

/// Substrings that are never offered for deletion even if they live under a
/// scanned root. Conservative protection against breaking active state.
const PROTECTED_SUBSTRINGS: &[&str] = &[
    "com.apple.containermanagerd",
    "CloudKit",
    "com.apple.cloudd",
    "FamilyCircle",
    // Browser service workers / extension state can break logins if cleared
    // mid-session; we keep the leveldb stores.
    "Service Worker/Database",
    "/IndexedDB",
];

fn h(rel: &str) -> PathBuf {
    safety::home().join(rel)
}

fn categories() -> Vec<CategoryDef> {
    vec![
        CategoryDef {
            id: "user_caches",
            label: "User & App Caches",
            description: "Regenerable caches written by apps under ~/Library/Caches.",
            risk: "safe",
            always_direct: false,
            sources: vec![Source::DirChildren(h("Library/Caches"))],
        },
        CategoryDef {
            id: "logs",
            label: "Logs",
            description: "Diagnostic logs under ~/Library/Logs.",
            risk: "safe",
            always_direct: false,
            sources: vec![Source::DirChildren(h("Library/Logs"))],
        },
        CategoryDef {
            id: "trash",
            label: "Trash",
            description: "Everything in your Trash, including the iCloud Drive trash.",
            risk: "safe",
            always_direct: true,
            sources: vec![
                Source::DirChildren(h(".Trash")),
                // Files deleted from iCloud Drive / iCloud Desktop land here, not
                // in ~/.Trash. Finder shows both as one "Trash".
                Source::DirChildren(h("Library/Mobile Documents/.Trash")),
            ],
        },
        CategoryDef {
            id: "dev_caches",
            label: "Developer Caches",
            description: "Re-downloadable caches from npm, yarn, gradle, cargo, Go, Xcode, etc.",
            risk: "safe",
            always_direct: false,
            sources: vec![
                Source::DirChildren(h("Library/Developer/Xcode/DerivedData")),
                Source::DirChildren(h("Library/Developer/Xcode/iOS DeviceSupport")),
                Source::DirChildren(h("Library/Developer/Xcode/iOS Device Logs")),
                Source::Single(h("Library/Developer/CoreSimulator/Caches")),
                Source::DirChildren(h(".cache")),
                Source::Single(h(".npm/_cacache")),
                Source::Single(h(".bun/install/cache")),
                Source::Single(h(".yarn/cache")),
                Source::Single(h(".gradle/caches")),
                Source::Single(h(".composer/cache")),
                Source::Single(h(".cargo/registry/cache")),
                Source::Single(h(".rustup/downloads")),
                Source::Single(h("go/pkg/mod/cache/download")),
            ],
        },
        CategoryDef {
            id: "app_code_caches",
            label: "App Code Caches",
            description:
                "Code/GPU/service-worker caches from Electron apps (Slack, Discord, VS Code, …).",
            risk: "safe",
            always_direct: false,
            sources: vec![
                Source::Glob(home_glob("Library/Application Support/*/Code Cache")),
                Source::Glob(home_glob("Library/Application Support/*/GPUCache")),
                Source::Glob(home_glob("Library/Application Support/*/Cache")),
                Source::Glob(home_glob(
                    "Library/Application Support/*/Service Worker/CacheStorage",
                )),
            ],
        },
        CategoryDef {
            id: "container_caches",
            label: "Sandboxed App Caches",
            description: "Caches inside sandboxed apps' containers (~/Library/Containers).",
            risk: "safe",
            always_direct: false,
            sources: vec![Source::Glob(home_glob(
                "Library/Containers/*/Data/Library/Caches/*",
            ))],
        },
        CategoryDef {
            id: "browser_caches",
            label: "Browser Caches",
            description: "Cache, Code Cache and GPU cache from web browsers.",
            risk: "safe",
            always_direct: false,
            sources: vec![
                Source::Glob(home_glob(
                    "Library/Application Support/Google/Chrome/*/Cache",
                )),
                Source::Glob(home_glob(
                    "Library/Application Support/Google/Chrome/*/Code Cache",
                )),
                Source::Glob(home_glob(
                    "Library/Application Support/Google/Chrome/*/GPUCache",
                )),
                Source::Glob(home_glob(
                    "Library/Application Support/com.microsoft.edgemac/*/Cache",
                )),
                Source::Glob(home_glob(
                    "Library/Application Support/BraveSoftware/Brave-Browser/*/Cache",
                )),
                Source::Glob(home_glob(
                    "Library/Application Support/Arc/User Data/*/Cache",
                )),
                Source::Glob(home_glob("Library/Application Support/Vivaldi/*/Cache")),
                Source::Glob(home_glob(
                    "Library/Application Support/com.operasoftware.Opera/*/Cache",
                )),
                Source::Glob(home_glob(
                    "Library/Application Support/Yandex/YandexBrowser/*/Cache",
                )),
            ],
        },
    ]
}

fn home_glob(rel: &str) -> String {
    safety::home().join(rel).to_string_lossy().to_string()
}

#[derive(Serialize, Clone)]
pub struct ScanEntry {
    /// Stable id == absolute path.
    pub id: String,
    pub path: String,
    pub name: String,
    pub size: u64,
}

#[derive(Serialize)]
pub struct CategoryResult {
    pub id: String,
    pub label: String,
    pub description: String,
    pub risk: String,
    pub always_direct: bool,
    pub total_size: u64,
    pub entries: Vec<ScanEntry>,
}

fn is_protected(path: &Path) -> bool {
    let s = path.to_string_lossy();
    PROTECTED_SUBSTRINGS.iter().any(|p| s.contains(p))
}

/// Resolve a source to its list of candidate target paths.
fn resolve_source(src: &Source) -> Vec<PathBuf> {
    match src {
        Source::DirChildren(dir) => match fs::read_dir(dir) {
            Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).collect(),
            Err(_) => Vec::new(),
        },
        Source::Single(p) => {
            if p.exists() {
                vec![p.clone()]
            } else {
                Vec::new()
            }
        }
        Source::Glob(pat) => match glob(pat) {
            Ok(paths) => paths.filter_map(|p| p.ok()).collect(),
            Err(_) => Vec::new(),
        },
    }
}

/// Scan every category. Sizing runs in parallel across entries.
/// Fast: enumerate every category's entries *without* sizing them (just
/// directory listing). Sizes are filled in afterwards by `size_paths`, so the
/// UI can render immediately and stream sizes in — walking a 3 GB cache to size
/// it must not block the whole view.
#[tauri::command]
pub async fn scan() -> Vec<CategoryResult> {
    tauri::async_runtime::spawn_blocking(scan_impl)
        .await
        .unwrap_or_default()
}

fn scan_impl() -> Vec<CategoryResult> {
    categories()
        .into_iter()
        .map(|cat| {
            let mut targets: Vec<PathBuf> = cat
                .sources
                .iter()
                .flat_map(resolve_source)
                .filter(|p| safety::is_deletable(p) && !is_protected(p))
                .collect();
            targets.sort();
            targets.dedup();

            let entries: Vec<ScanEntry> = targets
                .iter()
                .map(|p| ScanEntry {
                    id: p.to_string_lossy().to_string(),
                    path: p.to_string_lossy().to_string(),
                    name: display_name(p),
                    size: 0, // filled by size_paths
                })
                .collect();

            CategoryResult {
                id: cat.id.to_string(),
                label: cat.label.to_string(),
                description: cat.description.to_string(),
                risk: cat.risk.to_string(),
                always_direct: cat.always_direct,
                total_size: 0,
                entries,
            }
        })
        .collect()
}

#[derive(Serialize)]
pub struct PathSize {
    pub path: String,
    pub size: u64,
}

/// Compute on-disk sizes for the given paths, in parallel. Used to stream
/// per-category sizes after `scan`.
#[tauri::command]
pub async fn size_paths(paths: Vec<String>) -> Vec<PathSize> {
    tauri::async_runtime::spawn_blocking(move || size_paths_impl(paths))
        .await
        .unwrap_or_default()
}

fn size_paths_impl(paths: Vec<String>) -> Vec<PathSize> {
    paths
        .par_iter()
        .map(|p| PathSize {
            path: p.clone(),
            size: fsutil::dir_size(Path::new(p)),
        })
        .collect()
}

/// A nicer label for an entry: for nested cache dirs include the parent
/// (".../Chrome/Default/Cache" -> "Chrome – Default") otherwise the basename.
fn display_name(p: &Path) -> String {
    let name = p
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    if matches!(name.as_str(), "Cache" | "Code Cache" | "GPUCache" | "cache") {
        if let Some(parent) = p.parent().and_then(|x| x.file_name()) {
            return format!("{} – {}", parent.to_string_lossy(), name);
        }
    }
    name
}

#[derive(Deserialize)]
pub struct CleanRequest {
    /// Absolute paths the user chose to remove.
    pub paths: Vec<String>,
    /// When true, move to Trash; when false, delete directly.
    pub to_trash: bool,
}

#[derive(Serialize)]
pub struct CleanResult {
    pub removed: usize,
    pub freed: u64,
    pub failed: Vec<FailedItem>,
    /// True when at least one failure looked like a permission block, so the UI
    /// can point the user at Full Disk Access.
    pub needs_full_disk_access: bool,
}

#[derive(Serialize)]
pub struct FailedItem {
    pub path: String,
    pub error: String,
}

/// Delete the requested paths. Each path is re-validated against the safety
/// guard, so a forged path from the frontend cannot escape the allow-list.
#[tauri::command]
pub async fn clean(req: CleanRequest) -> CleanResult {
    tauri::async_runtime::spawn_blocking(move || clean_impl(req))
        .await
        .unwrap_or_else(|_| CleanResult {
            removed: 0,
            freed: 0,
            failed: Vec::new(),
            needs_full_disk_access: false,
        })
}

fn clean_impl(req: CleanRequest) -> CleanResult {
    let mut removed = 0usize;
    let mut freed = 0u64;
    let mut failed = Vec::new();

    for raw in req.paths {
        let path = PathBuf::from(&raw);
        let allowed = (safety::is_deletable(&path) || safety::is_project_artifact(&path))
            && !is_protected(&path)
            && !crate::protect::is_protected(&path);
        if !allowed {
            failed.push(FailedItem {
                path: raw,
                error: "blocked by safety guard".into(),
            });
            continue;
        }
        if !path.exists() {
            continue; // already gone; treat as success silently
        }
        let size = fsutil::dir_size(&path);
        // Items already in a Trash can't be "moved to Trash" — emptying the
        // Trash is always a direct removal regardless of the UI toggle.
        let home = safety::home();
        let in_trash = path.starts_with(home.join(".Trash"))
            || path.starts_with(home.join("Library/Mobile Documents/.Trash"));
        let to_trash = req.to_trash && !in_trash;
        match fsutil::delete(&path, to_trash) {
            Ok(_) => {
                removed += 1;
                freed += size;
                crate::cleanlog::record("clean", &raw, size, to_trash);
            }
            Err(error) => failed.push(FailedItem { path: raw, error }),
        }
    }

    let needs_full_disk_access = failed.iter().any(|f| fsutil::is_permission_error(&f.error));
    CleanResult {
        removed,
        freed,
        failed,
        needs_full_disk_access,
    }
}

// ─────────────────────── project build artifacts ───────────────────────

/// Which parent-manifest files mark a dir name as a real build artifact.
fn artifact_markers(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "node_modules" | "dist" | "build" | "out" | ".next" | ".nuxt" | ".turbo"
        | ".svelte-kit" | ".parcel-cache" => Some(&["package.json"]),
        "target" => Some(&["Cargo.toml", "pom.xml", "build.gradle", "build.gradle.kts"]),
        _ => None,
    }
}

/// Common directories where projects live; only existing ones are scanned.
/// Canonicalized + deduped because macOS's filesystem is case-insensitive
/// ("Code" and "code" are the same dir) — otherwise every artifact is found
/// once per spelling.
fn dev_roots() -> Vec<PathBuf> {
    let h = safety::home();
    let mut roots: Vec<PathBuf> = [
        "Developer",
        "Projects",
        "projects",
        "Code",
        "code",
        "dev",
        "Dev",
        "work",
        "Work",
        "src",
        "repos",
        "Repos",
        "git",
        "GitHub",
        "Sites",
        "www",
    ]
    .iter()
    .map(|d| h.join(d))
    .filter(|p| p.is_dir())
    .filter_map(|p| std::fs::canonicalize(p).ok())
    .collect();
    roots.sort();
    roots.dedup();
    roots
}

/// Walk the dev roots and collect build-artifact directories that sit next to a
/// real project manifest (so we never flag a random folder named "build").
fn find_project_artifacts() -> Vec<PathBuf> {
    let mut found = Vec::new();
    for root in dev_roots() {
        let walker = WalkDir::new(&root)
            .max_depth(7)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                // Don't descend into hidden dirs or artifact dirs themselves —
                // keeps the walk cheap (no traversing into node_modules).
                if e.depth() == 0 {
                    return true;
                }
                let n = e.file_name().to_str().unwrap_or("");
                !n.starts_with('.') && artifact_markers(n).is_none()
            });
        for entry in walker.filter_map(|e| e.ok()) {
            if !entry.file_type().is_dir() {
                continue;
            }
            let dir = entry.path();
            // Only inspect actual project roots.
            if !dir.join("package.json").exists()
                && !dir.join("Cargo.toml").exists()
                && !dir.join("pom.xml").exists()
                && !dir.join("build.gradle").exists()
            {
                continue;
            }
            for name in safety::PROJECT_ARTIFACT_NAMES {
                let candidate = dir.join(name);
                let markers = artifact_markers(name).unwrap_or(&[]);
                if candidate.is_dir() && markers.iter().any(|m| dir.join(m).exists()) {
                    found.push(candidate);
                }
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Scan for project build artifacts and size them. Separate from `scan` so the
/// (slower) recursive walk doesn't delay the main cache scan.
#[tauri::command]
pub async fn scan_projects() -> CategoryResult {
    tauri::async_runtime::spawn_blocking(scan_projects_impl)
        .await
        .unwrap_or_else(|_| empty_projects_category())
}

fn empty_projects_category() -> CategoryResult {
    CategoryResult {
        id: "project_artifacts".into(),
        label: "Project Build Artifacts".into(),
        description: "node_modules, target, dist… next to a project — safe to rebuild.".into(),
        risk: "caution".into(),
        always_direct: false,
        total_size: 0,
        entries: Vec::new(),
    }
}

fn scan_projects_impl() -> CategoryResult {
    let h = safety::home();
    let mut entries: Vec<ScanEntry> = find_project_artifacts()
        .par_iter()
        .map(|p| {
            // Display as "project/artifact" relative to home.
            let label = p
                .strip_prefix(&h)
                .unwrap_or(p)
                .to_string_lossy()
                .to_string();
            ScanEntry {
                id: p.to_string_lossy().to_string(),
                path: p.to_string_lossy().to_string(),
                name: format!("~/{label}"),
                size: fsutil::dir_size(p),
            }
        })
        .filter(|e| e.size > 0)
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.size));
    let total_size = entries.iter().map(|e| e.size).sum();

    CategoryResult {
        total_size,
        entries,
        ..empty_projects_category()
    }
}
