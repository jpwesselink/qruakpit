use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, Runtime, WebviewWindow};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flight {
    pub message: String,
    pub duration_ms: u64,
    #[serde(default)]
    pub sound: Option<bool>,
    #[serde(default)]
    pub sound_pack: Option<String>,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub head: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub font: Option<String>,
}

// NSWindowStyleMask bits.
const NS_WINDOW_STYLE_MASK_BORDERLESS: u64 = 0;
const NS_WINDOW_STYLE_MASK_NONACTIVATING_PANEL: u64 = 1 << 7;

// NSWindowCollectionBehavior bits.
const NS_WINDOW_COLLECTION_CAN_JOIN_ALL_SPACES: u64 = 1 << 0;
const NS_WINDOW_COLLECTION_STATIONARY: u64 = 1 << 4;
const NS_WINDOW_COLLECTION_IGNORES_CYCLE: u64 = 1 << 6;
const NS_WINDOW_COLLECTION_FULL_SCREEN_AUXILIARY: u64 = 1 << 8;

// NSWindowLevel constants.
const NS_FLOATING_WINDOW_LEVEL: i64 = 3;

#[cfg(target_os = "macos")]
extern "C" {
    fn object_setClass(
        obj: *mut objc::runtime::Object,
        cls: *const objc::runtime::Class,
    ) -> *const objc::runtime::Class;
}

#[cfg(target_os = "macos")]
pub fn configure_overlay_window<R: Runtime>(win: &WebviewWindow<R>) {
    use cocoa::base::{id, NO, YES};
    use objc::{class, msg_send, sel, sel_impl};

    let raw = match win.ns_window() {
        Ok(p) => p as id,
        Err(_) => return,
    };
    if raw.is_null() {
        return;
    }

    let _ = win.set_decorations(false);

    unsafe {
        // Upgrade the NSWindow's class to NSPanel so the nonactivating-panel bit
        // and the canBecomeKey override actually take effect. Without this the
        // overlay can steal focus and the floating/fullscreen-aux collection
        // behavior is less reliable.
        let panel_class = class!(NSPanel);
        let _ = object_setClass(raw as *mut _, panel_class);

        // Style mask: borderless + nonactivating panel. Re-apply after the
        // class swap so the panel reads the bit.
        let mask: u64 =
            NS_WINDOW_STYLE_MASK_BORDERLESS | NS_WINDOW_STYLE_MASK_NONACTIVATING_PANEL;
        let _: () = msg_send![raw, setStyleMask: mask];

        // Click-through.
        let _: () = msg_send![raw, setIgnoresMouseEvents: YES];

        // Transparency.
        let clear: id = msg_send![class!(NSColor), clearColor];
        let _: () = msg_send![raw, setBackgroundColor: clear];
        let _: () = msg_send![raw, setOpaque: NO];
        let _: () = msg_send![raw, setHasShadow: NO];

        // NSFloatingWindowLevel (matches upstream's 'floating' level).
        let _: () = msg_send![raw, setLevel: NS_FLOATING_WINDOW_LEVEL];

        // Visible on every space, every fullscreen app, stationary, out of cmd-tab.
        let behavior: u64 = NS_WINDOW_COLLECTION_CAN_JOIN_ALL_SPACES
            | NS_WINDOW_COLLECTION_STATIONARY
            | NS_WINDOW_COLLECTION_FULL_SCREEN_AUXILIARY
            | NS_WINDOW_COLLECTION_IGNORES_CYCLE;
        let _: () = msg_send![raw, setCollectionBehavior: behavior];

        // Don't steal focus when shown.
        let _: () = msg_send![raw, setHidesOnDeactivate: NO];
        let _: () = msg_send![raw, orderFrontRegardless];
    }

    // Span the full primary display.
    if let Ok(Some(m)) = win.primary_monitor() {
        let size = m.size();
        let _ = win.set_size(tauri::PhysicalSize::new(size.width, size.height));
        let _ = win.set_position(tauri::PhysicalPosition::new(0i32, 0i32));
    }
}

#[cfg(not(target_os = "macos"))]
pub fn configure_overlay_window<R: Runtime>(win: &WebviewWindow<R>) {
    let _ = win.set_always_on_top(true);
    let _ = win.set_ignore_cursor_events(true);
    let _ = win.show();
}

/// Triggers one flight on the overlay window.
///
/// Only emits the event; do NOT re-run any AppKit setup here. `fly_across` may
/// be called from any tokio task, and AppKit (NSWindow / NSPanel) calls require
/// the main thread. Window configuration happens once at startup.
pub fn fly_across<R: Runtime>(app: &tauri::AppHandle<R>, flight: Flight) {
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.emit_to("overlay", "flight:start", flight);
    }
}
