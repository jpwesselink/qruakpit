use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const PREFS_FILE: &str = "prefs.json";

/// Non-personal preferences. Calendar credentials live in the Keychain, not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    pub lead_minutes: u32,
    pub message_template: String,
    pub sound_enabled: bool,
    pub stay_signed_in: bool,
    pub launch_at_login: bool,
    pub target_display: String, // "cursor" | "primary"
    pub theme: String,
    pub flier: String,
    pub font: String,
    pub speed: String, // "normal" | "fast" | "ultra"
    pub fly_at_start: bool,
    pub sound_pack: String,
    pub flier_head: String,
    pub flier_color: String,
    #[serde(default)]
    pub eventkit_enabled_calendars: Vec<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            lead_minutes: 5,
            message_template: "{title} in {minutes} minutes".into(),
            sound_enabled: true,
            stay_signed_in: true,
            launch_at_login: false,
            target_display: "cursor".into(),
            theme: "classic".into(),
            flier: "duck-plane".into(),
            font: "system".into(),
            speed: "normal".into(),
            fly_at_start: false,
            sound_pack: "quack".into(),
            flier_head: "duck".into(),
            flier_color: "red".into(),
            eventkit_enabled_calendars: Vec::new(),
        }
    }
}

fn data_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("studio.qruakpit.desktop");
    let _ = fs::create_dir_all(&dir);
    dir
}

fn prefs_path() -> PathBuf {
    data_dir().join(PREFS_FILE)
}

pub fn load_prefs() -> Prefs {
    fs::read_to_string(prefs_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Prefs>(&s).ok())
        .unwrap_or_default()
}

pub fn save_prefs(prefs: &Prefs) {
    if let Ok(s) = serde_json::to_string_pretty(prefs) {
        let _ = fs::write(prefs_path(), s);
    }
}

// --- Keychain helpers ---

const KEYCHAIN_SERVICE: &str = "studio.qruakpit.desktop";

pub fn keychain_set(key: &str, value: &str) -> Result<(), keyring::Error> {
    keyring::Entry::new(KEYCHAIN_SERVICE, key)?.set_password(value)
}

pub fn keychain_get(key: &str) -> Option<String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, key)
        .ok()
        .and_then(|e| e.get_password().ok())
}

pub fn keychain_delete(key: &str) {
    if let Ok(e) = keyring::Entry::new(KEYCHAIN_SERVICE, key) {
        let _ = e.delete_credential();
    }
}
