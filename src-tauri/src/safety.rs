//! Path safety guard. Every deletion goes through `is_deletable` first.
//!
//! The rule is conservative: a target must live strictly *inside* one of a
//! small set of allow-listed roots (all under $HOME), and must never *be* one
//! of those roots. This makes it impossible to accidentally wipe $HOME, a
//! Library root, or anything outside the user's own caches/logs/trash.

use std::path::{Path, PathBuf};

/// Roots (relative to $HOME) whose *children* may be deleted.
const ALLOWED_RELATIVE_ROOTS: &[&str] = &[
    "Library/Caches",
    "Library/Logs",
    "Library/Application Support",
    "Library/Developer",
    "Library/Containers",
    ".Trash",
    "Library/Mobile Documents/.Trash",
    ".cache",
    ".npm",
    ".gradle",
    ".yarn",
    ".bun",
    ".composer",
    ".cargo/registry",
    ".rustup/downloads",
    "go/pkg/mod/cache",
];

pub fn home() -> PathBuf {
    // Fall back to a path that cannot match any real file, so a missing $HOME
    // makes every allow-list root un-matchable (scan finds nothing) rather than
    // collapsing the roots onto "/" and exposing system directories.
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/nonexistent-trashly-no-home"))
}

/// The set of absolute allow-listed roots for this machine.
fn allowed_roots() -> Vec<PathBuf> {
    let h = home();
    ALLOWED_RELATIVE_ROOTS.iter().map(|r| h.join(r)).collect()
}

/// Returns true when `target` may be deleted.
///
/// `target` is taken as-is (we deliberately do not canonicalize, so a symlink
/// inside a cache dir is removed as a link rather than chased to its real
/// location). We instead reject any path containing `..`.
pub fn is_deletable(target: &Path) -> bool {
    if !target.is_absolute() {
        return false;
    }
    if target.components().any(|c| c.as_os_str() == "..") {
        return false;
    }
    let roots = allowed_roots();
    for root in &roots {
        // Must be strictly inside a root (a proper descendant), never the root itself.
        if target.starts_with(root) && target != root.as_path() {
            return true;
        }
    }
    false
}

/// Validation for uninstall targets, which legitimately reach more places than
/// the cache allow-list: any folder/file under ~/Library (but not a Library
/// top-level root), or an application bundle under /Applications or
/// ~/Applications.
pub fn is_uninstall_target(target: &Path) -> bool {
    if !target.is_absolute() || target.components().any(|c| c.as_os_str() == "..") {
        return false;
    }
    let h = home();
    let lib = h.join("Library");
    // A descendant of ~/Library, at least two levels deep (skip the bare roots
    // like ~/Library/Preferences themselves).
    if target.starts_with(&lib) && target != lib && target.parent() != Some(lib.as_path()) {
        return true;
    }
    // An .app bundle in a standard Applications folder.
    let is_app = target.extension().map(|e| e == "app").unwrap_or(false);
    if is_app {
        let parent = target.parent();
        if parent == Some(Path::new("/Applications"))
            || parent == Some(h.join("Applications").as_path())
        {
            return true;
        }
    }
    // Known dev-tool data dirs in $HOME that an uninstall legitimately targets
    // (Android emulators, Gradle cache). Allowed as a whole or by descendant.
    for rel in UNINSTALL_HOME_DIRS {
        let root = h.join(rel);
        if target == root || target.starts_with(&root) {
            return true;
        }
    }
    false
}

/// Directory names that are regenerable project build artifacts.
pub const PROJECT_ARTIFACT_NAMES: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".turbo",
    ".svelte-kit",
    ".parcel-cache",
];

/// True when `target` is a build-artifact directory we may delete: it lives
/// under $HOME (but not in ~/Library) and its final path component is one of
/// the known artifact names. Deleting such a dir is recoverable by rebuilding.
pub fn is_project_artifact(target: &Path) -> bool {
    if !target.is_absolute() || target.components().any(|c| c.as_os_str() == "..") {
        return false;
    }
    let h = home();
    if !target.starts_with(&h) || target == h || target.starts_with(h.join("Library")) {
        return false;
    }
    let name = target.file_name().and_then(|s| s.to_str()).unwrap_or("");
    PROJECT_ARTIFACT_NAMES.contains(&name)
}

/// Extra $HOME directories (beyond ~/Library) that uninstall may remove —
/// known dev-tool / app data dirs referenced by `app_specific_extras`.
const UNINSTALL_HOME_DIRS: &[&str] = &[
    ".android",
    ".gradle",
    ".vscode",
    ".cursor",
    ".docker",
    ".orbstack",
    ".rd",
    ".kube",
];

// Critical $HOME subtrees that the user-file tools (Duplicate Finder) must never
// touch — secrets and the whole Library (handled by Clean/Uninstall only).
const USER_PROTECTED_PREFIXES: &[&str] = &[
    "Library",
    ".ssh",
    ".gnupg",
    ".aws",
    ".config/gcloud",
    ".kube",
    ".docker",
];

/// macOS bundle / library package extensions. Their *internals* must never be
/// touched by the user-file tools — deleting files inside a `.photoslibrary`,
/// `.app`, `.fcpbundle`… corrupts the library or app.
pub const PACKAGE_EXTS: &[&str] = &[
    "app",
    "bundle",
    "framework",
    "plugin",
    "kext",
    "prefpane",
    "qlgenerator",
    "mdimporter",
    "rtfd",
    "photoslibrary",
    "photolibrary",
    "aplibrary",
    "migrationlibrary",
    "musiclibrary",
    "tvlibrary",
    "imovielibrary",
    "theater",
    "fcpbundle",
    "pbproj",
    "xcodeproj",
    "wdgt",
];

/// True when any component of `target` is a macOS package/library bundle (so the
/// path is *inside* — or is — a `.app`, `.photoslibrary`, `.fcpbundle`, …).
pub fn is_in_package(target: &Path) -> bool {
    target.components().any(|c| {
        Path::new(c.as_os_str())
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| PACKAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
            .unwrap_or(false)
    })
}

// Standard top-level $HOME folders that must not be deleted *wholesale* (only
// their contents). Guards against a misclick wiping all of ~/Downloads.
const HOME_TOP_DIRS: &[&str] = &[
    "Documents",
    "Downloads",
    "Desktop",
    "Pictures",
    "Movies",
    "Music",
    "Public",
    "Applications",
    "Developer",
    "Projects",
    "Sites",
];

/// Validation for the user-file tools (Duplicate Finder): any file or folder
/// strictly inside $HOME, except secrets, the Library tree, and the bare standard
/// folders themselves. The user picks these explicitly in the UI, so we trust the
/// selection but still fence off the dangerous roots.
pub fn is_user_path(target: &Path) -> bool {
    if !target.is_absolute() || target.components().any(|c| c.as_os_str() == "..") {
        return false;
    }
    let h = home();
    if !target.starts_with(&h) || target == h {
        return false;
    }
    // Never reach inside a macOS package / media library (Photos, apps, FCP…).
    if is_in_package(target) {
        return false;
    }
    for p in USER_PROTECTED_PREFIXES {
        if target.starts_with(h.join(p)) {
            return false;
        }
    }
    for d in HOME_TOP_DIRS {
        if target == h.join(d) {
            return false;
        }
    }
    target.exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletable_rejects_roots_and_outside() {
        let h = home();
        assert!(!is_deletable(&h)); // $HOME itself
        assert!(!is_deletable(&h.join("Library/Caches"))); // a root itself
        assert!(!is_deletable(&h.join("Library"))); // a parent of a root
        assert!(!is_deletable(Path::new("/etc/passwd")));
        assert!(!is_deletable(Path::new("/"))); // filesystem root
        assert!(!is_deletable(&h.join("Documents/secret.txt"))); // outside allow-list
        assert!(is_deletable(&h.join("Library/Caches/com.example.app")));
        assert!(is_deletable(&h.join(".Trash/old.dmg")));
        assert!(is_deletable(&h.join("Library/Caches/x/y/z"))); // deep descendant
    }

    #[test]
    fn deletable_rejects_relative_and_parent_escapes() {
        let h = home();
        assert!(!is_deletable(Path::new("Library/Caches/x"))); // not absolute
                                                               // A `..` escape, even if it textually starts with an allowed root.
        assert!(!is_deletable(&h.join("Library/Caches/../../../etc/passwd")));
        assert!(!is_deletable(&h.join("Library/Caches/../.ssh")));
    }

    #[test]
    fn uninstall_target_scope() {
        let h = home();
        // Bare Library sub-roots are off-limits; their children are fair game.
        assert!(!is_uninstall_target(&h.join("Library")));
        assert!(!is_uninstall_target(&h.join("Library/Preferences")));
        assert!(is_uninstall_target(
            &h.join("Library/Preferences/com.example.plist")
        ));
        assert!(is_uninstall_target(
            &h.join("Library/Containers/com.example")
        ));
        // .app bundles only in standard Applications folders.
        assert!(is_uninstall_target(Path::new("/Applications/Foo.app")));
        assert!(is_uninstall_target(&h.join("Applications/Bar.app")));
        assert!(!is_uninstall_target(Path::new(
            "/System/Applications/Mail.app"
        )));
        assert!(!is_uninstall_target(&h.join("Documents/Evil.app")));
        // Known dev-tool data dirs, whole or descendant.
        assert!(is_uninstall_target(&h.join(".android")));
        assert!(is_uninstall_target(&h.join(".gradle/caches")));
        // Escapes and outside paths.
        assert!(!is_uninstall_target(&h.join("Library/../.ssh")));
        assert!(!is_uninstall_target(Path::new("/etc/hosts")));
    }

    #[test]
    fn project_artifact_scope() {
        let h = home();
        assert!(is_project_artifact(&h.join("dev/app/node_modules")));
        assert!(is_project_artifact(&h.join("Projects/x/target")));
        assert!(is_project_artifact(&h.join("code/site/.next")));
        // Wrong name, wrong place, or a root.
        assert!(!is_project_artifact(&h.join("dev/app/src"))); // not an artifact name
        assert!(!is_project_artifact(&h.join("Library/Caches/node_modules"))); // under Library
        assert!(!is_project_artifact(&h)); // $HOME itself
        assert!(!is_project_artifact(Path::new("/tmp/node_modules"))); // outside $HOME
        assert!(!is_project_artifact(&h.join("dev/../.ssh/node_modules"))); // `..` escape
    }

    #[test]
    fn user_path_scope() {
        let h = home();
        // Rejected regardless of existence: roots, secrets, Library, escapes.
        assert!(!is_user_path(&h));
        assert!(!is_user_path(&h.join("Downloads"))); // bare top-level folder
        assert!(!is_user_path(&h.join(".ssh/id_rsa")));
        assert!(!is_user_path(&h.join("Library/Caches/x")));
        assert!(!is_user_path(&h.join("Downloads/../.ssh"))); // `..` escape
        assert!(!is_user_path(Path::new("/etc/hosts"))); // outside $HOME
                                                         // Never inside a macOS package / media library.
        assert!(!is_user_path(
            &h.join("Pictures/Photos Library.photoslibrary/originals/x.jpg")
        ));
        assert!(!is_user_path(
            &h.join("Downloads/Some.app/Contents/MacOS/bin")
        ));
        assert!(is_in_package(&h.join("Movies/My.fcpbundle/CurrentVersion")));
    }
}
