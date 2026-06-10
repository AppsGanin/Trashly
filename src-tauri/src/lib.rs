mod clean;
mod cleanlog;
mod dupes;
mod finder;
mod fsutil;
mod optimize;
mod photos;
mod protect;
mod safety;
mod scanctl;
mod status;
mod tray;
mod uninstall;
mod userfiles;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Block default browser behaviours (context menu, reload, find, print, …).
    // In debug keep dev tools + reload so development still works.
    #[cfg(debug_assertions)]
    let prevent_default = tauri_plugin_prevent_default::debug();
    #[cfg(not(debug_assertions))]
    let prevent_default = tauri_plugin_prevent_default::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(prevent_default)
        .setup(|app| {
            protect::load(app.handle());
            cleanlog::init(app.handle());
            tray::setup(app)?;
            Ok(())
        })
        // Closing the window hides it (the app lives on in the menu bar);
        // quit from the tray to exit fully.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            clean::scan,
            clean::scan_projects,
            clean::size_paths,
            clean::clean,
            status::status,
            status::system_info,
            uninstall::list_apps,
            uninstall::app_leftovers,
            uninstall::app_icon,
            uninstall::uninstall,
            uninstall::is_app_running,
            finder::reveal_in_finder,
            finder::quick_look,
            optimize::list_optimizations,
            optimize::run_optimization,
            tray::get_tray_settings,
            tray::set_tray_settings,
            tray::tray_has_battery,
            dupes::dupe_roots,
            dupes::scan_duplicates,
            photos::scan_similar_photos,
            scanctl::cancel_scan,
            userfiles::remove_files,
            protect::get_protected_paths,
            protect::set_protected_paths,
            cleanlog::get_cleanup_log,
            cleanlog::clear_cleanup_log,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
