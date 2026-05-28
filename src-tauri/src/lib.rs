mod calendar;
mod ipc;
mod overlay;
mod scheduler;
mod store;
mod tray;

use std::sync::Arc;
use tauri::{Manager, RunEvent};
use tokio::sync::Mutex;

pub struct AppState {
    pub prefs: Mutex<store::Prefs>,
    pub calendars: Mutex<calendar::State>,
    pub scheduler: Mutex<scheduler::SchedulerHandle>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let prefs = store::load_prefs();
    let calendars = calendar::State::load(&prefs);
    let state = Arc::new(AppState {
        prefs: Mutex::new(prefs),
        calendars: Mutex::new(calendars),
        scheduler: Mutex::new(scheduler::SchedulerHandle::idle()),
    });

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(state.clone())
        .invoke_handler(tauri::generate_handler![
            ipc::prefs_get,
            ipc::prefs_set,
            ipc::cal_status,
            ipc::cal_connect,
            ipc::cal_disconnect,
            ipc::cal_configure,
            ipc::ical_list,
            ipc::ical_add,
            ipc::ical_remove,
            ipc::events_upcoming,
            ipc::flight_test,
            ipc::open_external,
            ipc::oauth_callback,
            ipc::eventkit_list_calendars,
            ipc::eventkit_set_enabled,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();

            // Configure the overlay window: NSPanel, click-through, above fullscreen apps.
            if let Some(win) = handle.get_webview_window("overlay") {
                overlay::configure_overlay_window(&win);
            }

            tray::install(&handle)?;
            tray::register_global_shortcut(&handle)?;

            // Settings window: hide on close instead of destroy, so the tray's
            // "Settings…" item can re-open the same instance later.
            if let Some(settings) = handle.get_webview_window("settings") {
                let win = settings.clone();
                settings.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = win.hide();
                    }
                });
                let _ = settings.show();
                let _ = settings.set_focus();
            }

            // Start scheduler.
            let state_for_sched = state.clone();
            tauri::async_runtime::spawn(async move {
                let handle = scheduler::start(handle.clone(), state_for_sched.clone()).await;
                *state_for_sched.scheduler.lock().await = handle;
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_app_handle, event| match event {
        RunEvent::ExitRequested { .. } => {}
        _ => {}
    });
}
