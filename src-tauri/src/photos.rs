//! Similar-photo finder. Unlike the exact duplicate finder, this matches images
//! that merely *look* alike (resized, re-compressed, edited screenshots…) using a
//! 64-bit dHash perceptual hash and clustering by Hamming distance.

use std::io::Cursor;
use std::os::macos::fs::MetadataExt as MacExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::UNIX_EPOCH;

use base64::Engine;
use image::{imageops::FilterType, DynamicImage, ImageFormat};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use walkdir::WalkDir;

use crate::{safety, scanctl};

/// Image extensions we can handle. HEIC/HEIF are decoded via the system `sips`.
const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "webp", "tif", "tiff", "heic", "heif",
];

/// Counter for unique HEIC scratch-file names.
static HEIC_SEQ: AtomicU64 = AtomicU64::new(0);

/// Hard cap on candidate images — clustering is O(n²), so bound the work.
const MAX_IMAGES: usize = 6000;

/// `st_flags` bit for iCloud-offloaded ("dataless") files — skip to avoid forcing a download.
const SF_DATALESS: u32 = 0x4000_0000;

#[derive(Serialize, Clone)]
pub struct PhotoFile {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: u64,
    /// Small PNG data-URL preview.
    pub thumb: String,
}

#[derive(Serialize)]
pub struct PhotoGroup {
    pub count: usize,
    /// Reclaimable if all but the largest are removed.
    pub wasted: u64,
    pub files: Vec<PhotoFile>,
}

#[derive(Serialize)]
pub struct PhotoResult {
    pub groups: Vec<PhotoGroup>,
    pub total_wasted: u64,
    pub scanned: usize,
    /// True when MAX_IMAGES was hit and some images were skipped.
    pub truncated: bool,
    /// Roots we couldn't read (usually a macOS privacy permission — needs access).
    pub unreadable: Vec<String>,
    /// Images skipped because they're iCloud-offloaded (dataless).
    pub skipped_icloud: usize,
}

#[derive(Deserialize)]
pub struct PhotoRequest {
    pub roots: Vec<String>,
    pub min_size: u64,
    /// Max Hamming distance to treat two images as similar (≈6 strict … 14 loose).
    pub threshold: u32,
}

const ROOTS: &[&str] = &["Pictures", "Desktop", "Downloads", "Documents"];

#[tauri::command]
pub async fn scan_similar_photos(app: AppHandle, req: PhotoRequest) -> PhotoResult {
    tauri::async_runtime::spawn_blocking(move || scan_impl(&app, req))
        .await
        .unwrap_or_else(|_| PhotoResult {
            groups: Vec::new(),
            total_wasted: 0,
            scanned: 0,
            truncated: false,
            unreadable: Vec::new(),
            skipped_icloud: 0,
        })
}

struct Img {
    file: PhotoFile,
    hash: u64,
}

fn scan_impl(app: &AppHandle, req: PhotoRequest) -> PhotoResult {
    scanctl::reset();
    let roots: Vec<PathBuf> = if req.roots.is_empty() {
        let h = safety::home();
        ROOTS
            .iter()
            .map(|r| h.join(r))
            .filter(|p| p.is_dir())
            .collect()
    } else {
        req.roots
            .into_iter()
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .collect()
    };
    let min_size = req.min_size.max(1);
    let threshold = req.threshold.min(32);

    // Collect candidate image paths.
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut unreadable: Vec<String> = Vec::new();
    let mut skipped_icloud = 0usize;
    for root in &roots {
        // A read_dir error on the root is almost always a macOS privacy block
        // (Desktop / Documents / Downloads need explicit access) — surface it.
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
            .filter_entry(|e| !safety::is_in_package(e.path()))
            .filter_map(|e| e.ok())
        {
            if scanctl::is_cancelled() {
                break;
            }
            if !entry.file_type().is_file() {
                continue;
            }
            if !is_image(entry.path()) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.len() < min_size {
                continue;
            }
            if meta.st_flags() & SF_DATALESS != 0 {
                skipped_icloud += 1;
                continue; // iCloud-offloaded
            }
            paths.push(entry.into_path());
            if paths.len().is_multiple_of(500) {
                scanctl::emit(app, "walk", paths.len(), 0);
            }
        }
    }
    let scanned = paths.len();
    let truncated = scanned > MAX_IMAGES;
    paths.truncate(MAX_IMAGES);

    // Decode + hash + thumbnail in parallel (with progress); undecodable drop out.
    let total = paths.len();
    let counter = AtomicUsize::new(0);
    let imgs: Vec<Img> = paths
        .into_par_iter()
        .filter_map(|p| {
            if scanctl::is_cancelled() {
                return None;
            }
            let img = load(&p);
            let n = counter.fetch_add(1, Ordering::Relaxed) + 1;
            if n.is_multiple_of(10) || n == total {
                scanctl::emit(app, "hash", n, total);
            }
            img
        })
        .collect();

    // Union-find clustering by Hamming distance ≤ threshold.
    let n = imgs.len();
    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        if scanctl::is_cancelled() {
            break;
        }
        for j in (i + 1)..n {
            if (imgs[i].hash ^ imgs[j].hash).count_ones() <= threshold {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                if a != b {
                    parent[a] = b;
                }
            }
        }
    }

    // Bucket indices by cluster root.
    let mut clusters: std::collections::HashMap<usize, Vec<usize>> =
        std::collections::HashMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        clusters.entry(r).or_default().push(i);
    }

    let mut groups: Vec<PhotoGroup> = clusters
        .into_values()
        .filter(|idxs| idxs.len() > 1)
        .map(|idxs| {
            let mut files: Vec<PhotoFile> = idxs.iter().map(|&i| imgs[i].file.clone()).collect();
            files.sort_by(|a, b| b.size.cmp(&a.size)); // largest (best) first
            let total: u64 = files.iter().map(|f| f.size).sum();
            let keep = files.first().map(|f| f.size).unwrap_or(0);
            PhotoGroup {
                count: files.len(),
                wasted: total - keep,
                files,
            }
        })
        .collect();

    groups.sort_by(|a, b| b.wasted.cmp(&a.wasted));
    let total_wasted = groups.iter().map(|g| g.wasted).sum();
    PhotoResult {
        groups,
        total_wasted,
        scanned,
        truncated,
        unreadable,
        skipped_icloud,
    }
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]]; // path halving
        i = parent[i];
    }
    i
}

fn is_image(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn load(p: &Path) -> Option<Img> {
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let img = if ext == "heic" || ext == "heif" {
        decode_heic(p)?
    } else {
        image::open(p).ok()?
    };
    let hash = dhash(&img);
    let thumb = thumbnail(&img)?;
    let meta = std::fs::metadata(p).ok()?;
    Some(Img {
        hash,
        file: PhotoFile {
            path: p.to_string_lossy().to_string(),
            name: p
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            size: meta.len(),
            modified: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0),
            thumb,
        },
    })
}

/// Decode a HEIC/HEIF via the system `sips` (no libheif dependency). We only
/// need a small image for the hash + thumbnail, so downscale to 512px first.
fn decode_heic(p: &Path) -> Option<DynamicImage> {
    let tmp = std::env::temp_dir().join(format!(
        "trashly-heic-{}.png",
        HEIC_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let ok = Command::new("/usr/bin/sips")
        .args(["-Z", "512", "-s", "format", "png"])
        .arg(p)
        .arg("--out")
        .arg(&tmp)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let out = if ok { image::open(&tmp).ok() } else { None };
    let _ = std::fs::remove_file(&tmp);
    out
}

/// 64-bit difference hash: resize to 9×8 grayscale, compare adjacent columns.
fn dhash(img: &DynamicImage) -> u64 {
    let small = img.resize_exact(9, 8, FilterType::Triangle).to_luma8();
    let mut hash = 0u64;
    let mut bit = 0;
    for y in 0..8 {
        for x in 0..8 {
            if small.get_pixel(x, y)[0] < small.get_pixel(x + 1, y)[0] {
                hash |= 1 << bit;
            }
            bit += 1;
        }
    }
    hash
}

fn thumbnail(img: &DynamicImage) -> Option<String> {
    let t = img.thumbnail(96, 96);
    let mut buf = Vec::new();
    t.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
        .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&buf)
    ))
}
