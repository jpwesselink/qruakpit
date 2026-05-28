use crate::calendar::{ical, ProviderStatus, UpcomingEvent};
use crate::overlay::{fly_across, Flight};
use crate::store::{save_prefs, Prefs};
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, State};
use tauri_plugin_shell::ShellExt;

/// Matches the upstream open-source build's "free" calendar limit: a single
/// source across all providers. Extending beyond that is an upstream Pro feature.
const SINGLE_CAL_MSG: &str =
    "Only one calendar source is supported in this build.";

/// Counts active calendar slots across all providers. Matches upstream's helper:
///   ical feed count + (google connected ? 1 : 0) + (icloud connected ? 1 : 0) + (eventkit has any enabled cal ? 1 : 0)
async fn active_calendar_count(state: &crate::calendar::State) -> usize {
    let mut n = state.ical.list().await.len();
    if state.google.status().connected {
        n += 1;
    }
    if state.icloud.status().connected {
        n += 1;
    }
    if state.eventkit.status().connected {
        n += 1;
    }
    n
}

#[tauri::command]
pub async fn prefs_get(state: State<'_, Arc<AppState>>) -> Result<Prefs, String> {
    Ok(state.prefs.lock().await.clone())
}

#[tauri::command]
pub async fn prefs_set(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    patch: serde_json::Value,
) -> Result<Prefs, String> {
    let mut prefs = state.prefs.lock().await;
    let prev_launch_at_login = prefs.launch_at_login;
    let mut current = serde_json::to_value(&*prefs).map_err(|e| e.to_string())?;
    if let (Some(cur), Some(p)) = (current.as_object_mut(), patch.as_object()) {
        for (k, v) in p {
            cur.insert(k.clone(), v.clone());
        }
    }
    let next: Prefs = serde_json::from_value(current).map_err(|e| e.to_string())?;
    save_prefs(&next);
    *prefs = next.clone();

    // Apply launch-at-login change via the autostart plugin.
    if next.launch_at_login != prev_launch_at_login {
        use tauri_plugin_autostart::ManagerExt;
        let manager = app.autolaunch();
        let _ = if next.launch_at_login {
            manager.enable()
        } else {
            manager.disable()
        };
    }

    Ok(next)
}

#[tauri::command]
pub async fn cal_status(state: State<'_, Arc<AppState>>) -> Result<Vec<ProviderStatus>, String> {
    Ok(state.calendars.lock().await.statuses())
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectParams {
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

#[tauri::command]
pub async fn cal_connect(
    state: State<'_, Arc<AppState>>,
    provider: String,
    params: ConnectParams,
) -> Result<Vec<ProviderStatus>, String> {
    let cals = state.calendars.lock().await;
    // Gate: only one calendar source total. Match upstream's free plan.
    // Reconnecting an already-connected provider is allowed.
    let already = match provider.as_str() {
        "google" => cals.google.status().connected,
        "icloud" => cals.icloud.status().connected,
        "eventkit" => cals.eventkit.status().connected,
        _ => false,
    };
    if !already && active_calendar_count(&cals).await >= 1 {
        return Err(SINGLE_CAL_MSG.into());
    }
    match provider.as_str() {
        "google" => cals.google.connect().await?,
        "icloud" => {
            cals.icloud
                .connect(
                    params.username.unwrap_or_default(),
                    params.password.unwrap_or_default(),
                )
                .await?
        }
        "eventkit" => {
            cals.eventkit.request_access().await?;
        }
        _ => return Err(format!("Unknown provider: {provider}")),
    };
    Ok(cals.statuses())
}

#[tauri::command]
pub async fn cal_disconnect(
    state: State<'_, Arc<AppState>>,
    provider: String,
) -> Result<Vec<ProviderStatus>, String> {
    let cals = state.calendars.lock().await;
    match provider.as_str() {
        "google" => cals.google.disconnect().await,
        "icloud" => cals.icloud.disconnect().await,
        "eventkit" => {
            cals.eventkit.set_enabled(Vec::new()).await;
        }
        _ => return Err(format!("Unknown provider: {provider}")),
    };
    Ok(cals.statuses())
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigureParams {
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
}

#[tauri::command]
pub async fn cal_configure(
    state: State<'_, Arc<AppState>>,
    provider: String,
    params: ConfigureParams,
) -> Result<Vec<ProviderStatus>, String> {
    let cals = state.calendars.lock().await;
    match provider.as_str() {
        "google" => {
            cals.google
                .set_creds(
                    params.client_id.unwrap_or_default(),
                    params.client_secret.unwrap_or_default(),
                )
                .await
        }
        _ => return Err(format!("Cannot configure provider: {provider}")),
    };
    Ok(cals.statuses())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventKitCalendarDto {
    pub id: String,
    pub title: String,
    pub source: String,
    pub enabled: bool,
}

#[tauri::command]
pub async fn eventkit_list_calendars(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<EventKitCalendarDto>, String> {
    let cals = state.calendars.lock().await;
    let all = cals.eventkit.list_calendars().await?;
    let enabled = cals.eventkit.enabled().await;
    Ok(all
        .into_iter()
        .map(|c| {
            let is_on = enabled.contains(&c.id);
            EventKitCalendarDto {
                id: c.id,
                title: c.title,
                source: c.source,
                enabled: is_on,
            }
        })
        .collect())
}

#[tauri::command]
pub async fn eventkit_set_enabled(
    state: State<'_, Arc<AppState>>,
    ids: Vec<String>,
) -> Result<Vec<ProviderStatus>, String> {
    {
        let cals = state.calendars.lock().await;
        // Going from "no eventkit calendars selected" to "one or more"
        // counts as adding a new calendar source. Block if another source
        // is already active.
        let going_on = !ids.is_empty() && !cals.eventkit.status().connected;
        if going_on && active_calendar_count(&cals).await >= 1 {
            return Err(SINGLE_CAL_MSG.into());
        }
        cals.eventkit.set_enabled(ids.clone()).await;
    }
    {
        let mut prefs = state.prefs.lock().await;
        prefs.eventkit_enabled_calendars = ids;
        crate::store::save_prefs(&prefs);
    }
    Ok(state.calendars.lock().await.statuses())
}


#[tauri::command]
pub async fn ical_list(state: State<'_, Arc<AppState>>) -> Result<Vec<ical::Feed>, String> {
    Ok(state.calendars.lock().await.ical.list().await)
}

#[tauri::command]
pub async fn ical_add(
    state: State<'_, Arc<AppState>>,
    url: String,
    name: Option<String>,
) -> Result<Vec<ical::Feed>, String> {
    let cals = state.calendars.lock().await;
    if active_calendar_count(&cals).await >= 1 {
        return Err(SINGLE_CAL_MSG.into());
    }
    cals.ical.add(url, name).await
}

#[tauri::command]
pub async fn ical_remove(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<Vec<ical::Feed>, String> {
    Ok(state.calendars.lock().await.ical.remove(id).await)
}

#[tauri::command]
pub async fn events_upcoming(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<UpcomingEvent>, String> {
    Ok(state.calendars.lock().await.list_upcoming(120).await)
}

#[tauri::command]
pub async fn flight_test(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<bool, String> {
    let prefs = state.prefs.lock().await.clone();
    fly_across(
        &app,
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
    Ok(true)
}

#[tauri::command]
pub async fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("only http(s) URLs are allowed".into());
    }
    app.shell()
        .open(url, None)
        .map_err(|e| e.to_string())
}

/// Stub for the localhost OAuth callback. Real implementation will be
/// invoked by the loopback HTTP listener that each OAuth provider spawns.
#[tauri::command]
pub async fn oauth_callback(_provider: String, _code: String) -> Result<(), String> {
    Err("oauth_callback wiring pending".into())
}
