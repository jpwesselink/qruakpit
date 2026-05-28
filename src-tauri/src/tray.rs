use crate::overlay::{fly_across, Flight};
use crate::AppState;
use std::sync::Arc;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Wry,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

const TRAY_ICON_BYTES: &[u8] = include_bytes!("../icons/tray.png");

pub fn install(app: &AppHandle<Wry>) -> tauri::Result<()> {
    let send_test = MenuItem::with_id(app, "send-test", "Send test flight", true, None::<&str>)?;
    let open_settings =
        MenuItem::with_id(app, "open-settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Qruakpit", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[&send_test, &open_settings, &quit])?;

    let icon = Image::from_bytes(TRAY_ICON_BYTES).unwrap_or_else(|_| {
        // Fallback: 16x16 transparent. Won't render but avoids a crash.
        let blank = vec![0u8; 16 * 16 * 4];
        Image::new_owned(blank, 16, 16)
    });

    let _tray = TrayIconBuilder::with_id("main")
        .icon(icon)
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "send-test" => trigger_test_flight(app),
            "open-settings" => {
                if let Some(w) = app.get_webview_window("settings") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|_tray, _event| {
            if let TrayIconEvent::DoubleClick { .. } = _event {
                // intentionally empty
            }
        })
        .build(app)?;

    Ok(())
}

pub fn register_global_shortcut(app: &AppHandle<Wry>) -> tauri::Result<()> {
    let app = app.clone();
    let shortcut = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyD);
    app.global_shortcut()
        .on_shortcut(shortcut, move |app_handle, _sc, event| {
            if event.state == ShortcutState::Pressed {
                trigger_test_flight(app_handle);
            }
        })
        .map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!(e)))?;
    Ok(())
}

fn trigger_test_flight<R: tauri::Runtime>(app: &AppHandle<R>) {
    let app_handle = app.clone();
    let state = match app.try_state::<Arc<AppState>>() {
        Some(s) => s.inner().clone(),
        None => return,
    };
    tauri::async_runtime::spawn(async move {
        let prefs = state.prefs.lock().await.clone();
        fly_across(
            &app_handle,
            Flight {
                message: "Call with Edwin in 5 minutes".into(),
                duration_ms: 9000,
                sound: Some(prefs.sound_enabled),
                sound_pack: Some(prefs.sound_pack),
                theme: Some(prefs.theme),
                head: Some(prefs.flier_head),
                color: Some(prefs.flier_color),
                font: Some(prefs.font),
            },
        );
    });
}
