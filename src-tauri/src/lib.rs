mod clean;
mod fsutil;
mod optimize;
mod safety;
mod status;
mod tray;
mod uninstall;

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
        .plugin(prevent_default)
        .setup(|app| {
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
            optimize::list_optimizations,
            optimize::run_optimization,
            tray::get_tray_settings,
            tray::set_tray_settings,
            tray::tray_has_battery,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
