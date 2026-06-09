//! Menu-bar tray icon with live system stats. Which metrics appear in the
//! menu-bar title and in the dropdown is user-configurable (Settings tab) and
//! persisted to disk.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sysinfo::{Disks, System};
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{App, AppHandle, Manager, State, Wry};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Metric {
    Cpu,
    Memory,
    Disk,
    Battery,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct TraySettings {
    /// Metrics shown in the menu-bar title (left → right).
    pub title: Vec<Metric>,
    /// Metrics listed in the dropdown.
    pub menu: Vec<Metric>,
}

impl Default for TraySettings {
    fn default() -> Self {
        Self {
            title: vec![Metric::Cpu],
            menu: vec![Metric::Cpu, Metric::Memory, Metric::Disk],
        }
    }
}

/// A live dropdown row: the metric it shows and its menu-item handle.
type MetricRow = (Metric, MenuItem<Wry>);

/// Shared state: current settings, the live dropdown row handles, and the menu
/// itself (kept alive while it's installed on the tray).
pub struct TrayRuntime {
    settings: TraySettings,
    rows: Vec<MetricRow>,
    _menu: Menu<Wry>,
}

#[derive(Clone, Copy, Default)]
struct Stats {
    cpu: f32,
    mem: f32,
    disk: Option<f32>,
    battery: Option<u8>,
}

// ─────────────────────────── formatting ───────────────────────────

fn short(m: Metric, s: &Stats) -> String {
    match m {
        Metric::Cpu => format!("CPU {:.0}%", s.cpu),
        Metric::Memory => format!("RAM {:.0}%", s.mem),
        Metric::Disk => s.disk.map_or("Disk —".into(), |d| format!("Disk {d:.0}%")),
        Metric::Battery => s.battery.map_or("Bat —".into(), |b| format!("Bat {b}%")),
    }
}

fn long(m: Metric, s: &Stats) -> String {
    match m {
        Metric::Cpu => format!("CPU      {:.0}%", s.cpu),
        Metric::Memory => format!("Memory   {:.0}%", s.mem),
        Metric::Disk => s
            .disk
            .map_or("Disk     —".into(), |d| format!("Disk     {d:.0}%")),
        Metric::Battery => s
            .battery
            .map_or("Battery  —".into(), |b| format!("Battery  {b}%")),
    }
}

// ─────────────────────────── persistence ───────────────────────────

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("tray-settings.json"))
}

fn load_settings(app: &AppHandle) -> TraySettings {
    settings_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_settings(app: &AppHandle, s: &TraySettings) {
    if let Some(p) = settings_path(app) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(s) {
            let _ = std::fs::write(p, json);
        }
    }
}

// ─────────────────────────── menu building ───────────────────────────

fn build_menu(
    app: &AppHandle,
    settings: &TraySettings,
) -> tauri::Result<(Menu<Wry>, Vec<MetricRow>)> {
    let placeholder = Stats::default();
    let has_bat = battery_pct().is_some();
    let mut rows = Vec::new();
    for m in &settings.menu {
        // No battery on desktop Macs (iMac / mini / Studio / Pro) → drop the row.
        if *m == Metric::Battery && !has_bat {
            continue;
        }
        let item = MenuItem::with_id(
            app,
            format!("m-{m:?}"),
            long(*m, &placeholder),
            false,
            None::<&str>,
        )?;
        rows.push((*m, item));
    }
    let sep = PredefinedMenuItem::separator(app)?;
    let show = MenuItem::with_id(app, "show", "Show Trashly", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Trashly", true, None::<&str>)?;

    let mut refs: Vec<&dyn IsMenuItem<Wry>> = rows
        .iter()
        .map(|(_, i)| i as &dyn IsMenuItem<Wry>)
        .collect();
    refs.push(&sep);
    refs.push(&show);
    refs.push(&quit);
    let menu = Menu::with_items(app, &refs)?;
    Ok((menu, rows))
}

// ─────────────────────────── setup + updater ───────────────────────────

pub fn setup(app: &App) -> tauri::Result<()> {
    let handle = app.handle().clone();
    let settings = load_settings(&handle);
    let (menu, rows) = build_menu(&handle, &settings)?;

    // Monochrome template icon → macOS auto-tints it for light/dark menu bars,
    // matching the native status-bar items (Wi-Fi, battery…).
    let icon = tauri::image::Image::from_bytes(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/icons/tray@2x.png"
    )))?;

    TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(true)
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    app.manage(Mutex::new(TrayRuntime {
        settings,
        rows,
        _menu: menu,
    }));

    let h = handle.clone();
    std::thread::spawn(move || updater(h));
    Ok(())
}

fn updater(handle: AppHandle) {
    let mut sys = System::new();
    loop {
        // Sample off the main thread.
        sys.refresh_cpu_all();
        std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
        sys.refresh_cpu_all();
        sys.refresh_memory();
        let total = sys.total_memory();
        let stats = Stats {
            cpu: sys.global_cpu_usage(),
            mem: if total > 0 {
                sys.used_memory() as f32 / total as f32 * 100.0
            } else {
                0.0
            },
            disk: disk_used_pct(),
            battery: battery_pct(),
        };

        // Tray / menu mutations must happen on the main thread (AppKit).
        let h = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            let state = h.state::<Mutex<TrayRuntime>>();
            let Ok(rt) = state.lock() else { return };
            if let Some(tray) = h.tray_by_id("main") {
                let parts: Vec<String> = rt
                    .settings
                    .title
                    .iter()
                    // Skip battery in the title when the machine has none.
                    .filter(|m| !(matches!(m, Metric::Battery) && stats.battery.is_none()))
                    .map(|m| short(*m, &stats))
                    .collect();
                // Always pass a string (empty → icon only); None can no-op on
                // some platforms and leave a stale title.
                let _ = tray.set_title(Some(parts.join("  ")));
                let tip = if parts.is_empty() {
                    "Trashly".to_string()
                } else {
                    format!("Trashly · {}", parts.join(" · "))
                };
                let _ = tray.set_tooltip(Some(tip));
            }
            for (m, item) in &rt.rows {
                let _ = item.set_text(long(*m, &stats));
            }
        });

        std::thread::sleep(Duration::from_secs(3));
    }
}

fn disk_used_pct() -> Option<f32> {
    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .find(|d| d.mount_point() == std::path::Path::new("/"))
        .filter(|d| d.total_space() > 0)
        .map(|d| (d.total_space() - d.available_space()) as f32 / d.total_space() as f32 * 100.0)
}

fn battery_pct() -> Option<u8> {
    let out = Command::new("/usr/bin/pmset")
        .args(["-g", "batt"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.contains('%'))?;
    line.split('%')
        .next()
        .and_then(|s| s.rsplit(|c: char| !c.is_ascii_digit()).next())
        .and_then(|s| s.parse().ok())
}

// ─────────────────────────── commands ───────────────────────────

#[tauri::command]
pub fn get_tray_settings(state: State<Mutex<TrayRuntime>>) -> TraySettings {
    state
        .lock()
        .map(|rt| rt.settings.clone())
        .unwrap_or_default()
}

/// Whether this Mac has a battery — Settings hides the Battery option if not.
#[tauri::command]
pub fn tray_has_battery() -> bool {
    battery_pct().is_some()
}

// Sync command → runs on the main thread, so we can rebuild & swap the tray
// menu directly (AppKit-safe) instead of deferring it.
#[tauri::command]
pub fn set_tray_settings(app: AppHandle, settings: TraySettings) -> Result<(), String> {
    save_settings(&app, &settings);
    let (menu, rows) = build_menu(&app, &settings).map_err(|e| e.to_string())?;
    if let Some(tray) = app.tray_by_id("main") {
        tray.set_menu(Some(menu.clone()))
            .map_err(|e| e.to_string())?;
    }
    if let Ok(mut rt) = app.state::<Mutex<TrayRuntime>>().lock() {
        rt.settings = settings;
        rt.rows = rows;
        rt._menu = menu;
    }
    Ok(())
}
