//! Optimize: one-shot macOS maintenance tasks. Privileged tasks are run through
//! osascript's "with administrator privileges", which shows the native auth
//! dialog instead of needing an embedded sudo.

use std::process::Command;

use serde::Serialize;

struct Task {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    needs_admin: bool,
    /// Shell command line executed via `bash -c` (or osascript for admin).
    cmd: &'static str,
}

const TASKS: &[Task] = &[
    Task {
        id: "rebuild_launch_services",
        label: "Rebuild Launch Services",
        description: "Fixes duplicate or wrong 'Open With' entries.",
        needs_admin: false,
        cmd: "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -r -domain local -domain system -domain user; echo rebuilt",
    },
    Task {
        id: "quicklook_cache",
        label: "Reset QuickLook cache",
        description: "Rebuilds Finder preview thumbnails.",
        needs_admin: false,
        cmd: "qlmanage -r cache",
    },
    Task {
        id: "font_cache",
        label: "Clear user font cache",
        description: "Removes the per-user font database to fix font glitches.",
        needs_admin: false,
        cmd: "atsutil databases -removeUser",
    },
    Task {
        id: "brew_cleanup",
        label: "Homebrew cleanup",
        description: "Removes old formula versions and Homebrew's download cache.",
        needs_admin: false,
        cmd: "b=$(command -v brew); for p in /opt/homebrew/bin/brew /usr/local/bin/brew; do [ -z \"$b\" ] && [ -x \"$p\" ] && b=\"$p\"; done; [ -z \"$b\" ] && { echo 'Homebrew not installed'; exit 1; }; \"$b\" cleanup -s 2>&1 | tail -15; echo done",
    },
    Task {
        id: "system_caches",
        label: "Clear system caches & logs",
        description: "Removes /Library/Caches and /Library/Logs (system-wide).",
        needs_admin: true,
        cmd: "find /Library/Caches /Library/Logs -mindepth 1 -maxdepth 1 -exec rm -rf {} + 2>/dev/null; echo cleared",
    },
    Task {
        id: "flush_dns",
        label: "Flush DNS cache",
        description: "Clears the resolver cache and restarts mDNSResponder.",
        needs_admin: true,
        cmd: "dscacheutil -flushcache; killall -HUP mDNSResponder",
    },
    Task {
        id: "purge_memory",
        label: "Purge inactive memory",
        description: "Frees inactive memory pages and disk cache.",
        needs_admin: true,
        cmd: "purge",
    },
    Task {
        id: "spotlight_reindex",
        label: "Rebuild Spotlight index",
        description: "Erases and reindexes the main volume (runs in background).",
        needs_admin: true,
        cmd: "mdutil -E / >/dev/null 2>&1; echo reindex-scheduled",
    },
];

#[derive(Serialize)]
pub struct OptimizationInfo {
    pub id: String,
    pub label: String,
    pub description: String,
    pub needs_admin: bool,
}

/// Whether a task's prerequisite tool is present — tasks that need a tool the
/// machine doesn't have (e.g. Homebrew) are hidden rather than shown to fail.
fn is_available(id: &str) -> bool {
    match id {
        "brew_cleanup" => ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
            .iter()
            .any(|p| std::path::Path::new(p).exists()),
        _ => true, // built-in macOS tools are always present
    }
}

#[tauri::command]
pub fn list_optimizations() -> Vec<OptimizationInfo> {
    TASKS
        .iter()
        .filter(|t| is_available(t.id))
        .map(|t| OptimizationInfo {
            id: t.id.to_string(),
            label: t.label.to_string(),
            description: t.description.to_string(),
            needs_admin: t.needs_admin,
        })
        .collect()
}

#[derive(Serialize)]
pub struct RunResult {
    pub success: bool,
    pub output: String,
}

#[tauri::command]
pub async fn run_optimization(id: String) -> RunResult {
    tauri::async_runtime::spawn_blocking(move || run_optimization_impl(id))
        .await
        .unwrap_or_else(|_| RunResult {
            success: false,
            output: "task panicked".into(),
        })
}

fn run_optimization_impl(id: String) -> RunResult {
    let Some(task) = TASKS.iter().find(|t| t.id == id) else {
        return RunResult {
            success: false,
            output: format!("unknown task: {id}"),
        };
    };

    let output = if task.needs_admin {
        // Escape for the AppleScript string literal (backslash first, then
        // quotes and control chars that would otherwise break the literal).
        let escaped = task
            .cmd
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\t', "\\t");
        let script = format!(
            "do shell script \"{}\" with administrator privileges",
            escaped
        );
        Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(script)
            .output()
    } else {
        Command::new("/bin/bash").arg("-c").arg(task.cmd).output()
    };

    match output {
        Ok(o) => {
            let mut text = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !o.status.success() {
                let err = String::from_utf8_lossy(&o.stderr);
                if !err.trim().is_empty() {
                    text = format!("{text}\n{}", err.trim());
                }
            }
            RunResult {
                success: o.status.success(),
                output: if text.is_empty() {
                    "Done.".into()
                } else {
                    text
                },
            }
        }
        Err(e) => RunResult {
            success: false,
            output: e.to_string(),
        },
    }
}
