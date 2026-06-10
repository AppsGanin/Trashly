//! Content-aware duplicate finder. Files are grouped by size first (cheap), then
//! identical-size candidates are confirmed by a full BLAKE3 content hash — so
//! matches are by *content*, never by name.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::os::macos::fs::MetadataExt as MacExt;
use std::os::unix::fs::MetadataExt as UnixExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::UNIX_EPOCH;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use walkdir::WalkDir;

use crate::{safety, scanctl};

/// `st_flags` bit set on iCloud-offloaded ("dataless") files. Reading them would
/// force a download, so we skip them entirely.
const SF_DATALESS: u32 = 0x4000_0000;

#[derive(Serialize, Clone)]
pub struct DupeFile {
    pub path: String,
    pub name: String,
    /// Last-modified time, epoch seconds (UI keeps the oldest by default).
    pub modified: u64,
}

#[derive(Serialize)]
pub struct DupeGroup {
    pub hash: String,
    /// Size of one file in the group.
    pub size: u64,
    pub count: usize,
    /// Reclaimable bytes = (count - 1) * size.
    pub wasted: u64,
    pub files: Vec<DupeFile>,
}

#[derive(Serialize)]
pub struct DupeResult {
    pub groups: Vec<DupeGroup>,
    pub total_wasted: u64,
    pub scanned: usize,
    /// Roots we couldn't read (usually a macOS privacy permission).
    pub unreadable: Vec<String>,
    /// Files skipped because they're iCloud-offloaded (dataless).
    pub skipped_icloud: usize,
}

/// A selectable search root for the UI.
#[derive(Serialize)]
pub struct RootInfo {
    pub key: String,
    pub label: String,
    pub path: String,
}

const ROOTS: &[(&str, &str)] = &[
    ("Downloads", "Downloads"),
    ("Documents", "Documents"),
    ("Desktop", "Desktop"),
    ("Pictures", "Pictures"),
    ("Movies", "Movies"),
    ("Music", "Music"),
];

/// The content folders that exist on this machine, for the UI's root toggles.
#[tauri::command]
pub fn dupe_roots() -> Vec<RootInfo> {
    let h = safety::home();
    ROOTS
        .iter()
        .map(|(key, rel)| (key, h.join(rel)))
        .filter(|(_, p)| p.is_dir())
        .map(|(key, p)| RootInfo {
            key: key.to_string(),
            label: key.to_string(),
            path: p.to_string_lossy().to_string(),
        })
        .collect()
}

#[derive(Deserialize)]
pub struct DupeRequest {
    /// Absolute roots to search; empty → all existing content folders.
    pub roots: Vec<String>,
    /// Ignore files smaller than this (bytes).
    pub min_size: u64,
}

#[tauri::command]
pub async fn scan_duplicates(app: AppHandle, req: DupeRequest) -> DupeResult {
    tauri::async_runtime::spawn_blocking(move || scan_impl(&app, req))
        .await
        .unwrap_or_else(|_| DupeResult {
            groups: Vec::new(),
            total_wasted: 0,
            scanned: 0,
            unreadable: Vec::new(),
            skipped_icloud: 0,
        })
}

fn scan_impl(app: &AppHandle, req: DupeRequest) -> DupeResult {
    scanctl::reset();
    let roots: Vec<PathBuf> = if req.roots.is_empty() {
        let h = safety::home();
        ROOTS
            .iter()
            .map(|(_, r)| h.join(r))
            .filter(|p| p.is_dir())
            .collect()
    } else {
        req.roots
            .into_iter()
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .collect()
    };
    let min_size = req.min_size.max(1); // never group zero-byte files

    // 1. bucket every regular file by size — only same-size files can be dupes.
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut scanned = 0usize;
    let mut unreadable: Vec<String> = Vec::new();
    let mut skipped_icloud = 0usize;
    for root in &roots {
        // A read_dir error is almost always a macOS privacy block (Desktop /
        // Documents / Downloads need explicit access) — surface it.
        if std::fs::read_dir(root).is_err() {
            unreadable.push(
                root.file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| root.to_string_lossy().to_string()),
            );
            continue;
        }
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            // Don't descend into .app / .photoslibrary / .fcpbundle … packages.
            .filter_entry(|e| !safety::is_in_package(e.path()))
            .filter_map(|e| e.ok())
        {
            if scanctl::is_cancelled() {
                break;
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.len() < min_size {
                continue;
            }
            if meta.st_flags() & SF_DATALESS != 0 {
                skipped_icloud += 1;
                continue; // iCloud-offloaded — don't force a download
            }
            scanned += 1;
            if scanned.is_multiple_of(500) {
                scanctl::emit(app, "walk", scanned, 0);
            }
            by_size
                .entry(meta.len())
                .or_default()
                .push(entry.into_path());
        }
    }

    // 2. hash only the size-collision candidates, in parallel (with progress).
    let candidate_files: Vec<(u64, PathBuf)> = by_size
        .into_iter()
        .filter(|(_, v)| v.len() > 1)
        .flat_map(|(size, paths)| paths.into_iter().map(move |p| (size, p)))
        .collect();
    let total = candidate_files.len();
    let counter = AtomicUsize::new(0);
    let hashed: Vec<(u64, String, PathBuf)> = candidate_files
        .into_par_iter()
        .filter_map(|(size, p)| {
            if scanctl::is_cancelled() {
                return None;
            }
            let h = hash_file(&p)?;
            let n = counter.fetch_add(1, Ordering::Relaxed) + 1;
            if n.is_multiple_of(20) || n == total {
                scanctl::emit(app, "hash", n, total);
            }
            Some((size, h, p))
        })
        .collect();

    // 3. group by (size, hash); a real duplicate set has > 1 member.
    let mut by_hash: HashMap<(u64, String), Vec<PathBuf>> = HashMap::new();
    for (size, h, p) in hashed {
        by_hash.entry((size, h)).or_default().push(p);
    }

    let mut groups: Vec<DupeGroup> = Vec::new();
    for ((size, hash), mut paths) in by_hash {
        if paths.len() < 2 {
            continue;
        }
        paths.sort();
        // Collapse hardlinks: several names → one inode frees no space, so keep
        // a single path per (device, inode).
        let mut seen: HashSet<(u64, u64)> = HashSet::new();
        paths.retain(|p| {
            std::fs::metadata(p)
                .map(|m| seen.insert((m.dev(), m.ino())))
                .unwrap_or(true)
        });
        if paths.len() < 2 {
            continue; // all the same physical file
        }
        let files: Vec<DupeFile> = paths
            .iter()
            .map(|p| DupeFile {
                path: p.to_string_lossy().to_string(),
                name: p
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default(),
                modified: modified_secs(p),
            })
            .collect();
        let count = files.len();
        groups.push(DupeGroup {
            hash,
            size,
            count,
            wasted: size * (count as u64 - 1),
            files,
        });
    }

    groups.sort_by(|a, b| b.wasted.cmp(&a.wasted));
    let total_wasted = groups.iter().map(|g| g.wasted).sum();
    DupeResult {
        groups,
        total_wasted,
        scanned,
        unreadable,
        skipped_icloud,
    }
}

/// Stream a file through BLAKE3 (chunked, so large media doesn't blow up RAM).
fn hash_file(p: &Path) -> Option<String> {
    let mut f = std::fs::File::open(p).ok()?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(hasher.finalize().to_hex().to_string())
}

fn modified_secs(p: &Path) -> u64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
