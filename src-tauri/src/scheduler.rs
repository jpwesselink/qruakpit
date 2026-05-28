use crate::overlay::{fly_across, Flight};
use crate::AppState;
use std::collections::HashSet;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};

const POLL_SECS: u64 = 60;
const TICK_SECS: u64 = 15;
const FIRE_WINDOW_MS: i64 = 90_000;

pub struct SchedulerHandle {
    _stop: Option<tokio::task::JoinHandle<()>>,
}

impl SchedulerHandle {
    pub fn idle() -> Self {
        Self { _stop: None }
    }
}

#[derive(Default)]
struct State {
    upcoming: Vec<crate::calendar::UpcomingEvent>,
    fired: HashSet<String>,
}

pub async fn start(handle: AppHandle, app_state: Arc<AppState>) -> SchedulerHandle {
    let inner = Arc::new(Mutex::new(State::default()));

    let refresh_inner = inner.clone();
    let refresh_state = app_state.clone();
    let refresh = tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(POLL_SECS));
        tick.tick().await; // immediate
        loop {
            refresh_inner_loop(&refresh_inner, &refresh_state).await;
            tick.tick().await;
        }
    });

    let tick_inner = inner.clone();
    let tick_handle = handle.clone();
    let tick_state = app_state.clone();
    let tick_task = tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(TICK_SECS));
        loop {
            tick.tick().await;
            tick_once(&tick_handle, &tick_inner, &tick_state).await;
        }
    });

    // Park both tasks under one join handle so dropping cancels them.
    let combined = tokio::spawn(async move {
        let _ = tokio::join!(refresh, tick_task);
    });

    SchedulerHandle { _stop: Some(combined) }
}

async fn refresh_inner_loop(inner: &Arc<Mutex<State>>, app_state: &Arc<AppState>) {
    let cals = app_state.calendars.lock().await;
    let upcoming = cals.list_upcoming(60).await;
    drop(cals);

    let mut s = inner.lock().await;
    let ids: HashSet<String> = upcoming.iter().map(|e| e.id.clone()).collect();
    s.fired
        .retain(|key| ids.iter().any(|id| key.starts_with(&format!("{id}:"))));
    s.upcoming = upcoming;
}

async fn tick_once(
    handle: &AppHandle,
    inner: &Arc<Mutex<State>>,
    app_state: &Arc<AppState>,
) {
    let prefs = app_state.prefs.lock().await.clone();
    let now = chrono::Utc::now().timestamp_millis();
    let lead_ms = prefs.lead_minutes as i64 * 60_000;

    let mut s = inner.lock().await;
    let events = s.upcoming.clone();

    for ev in events.iter() {
        // 1) The lead-time reminder.
        let trigger_at = ev.start - lead_ms;
        let lead_key = format!("{}:{}", ev.id, prefs.lead_minutes);
        let lead_due =
            now >= trigger_at && now < trigger_at + FIRE_WINDOW_MS && ev.start > now;
        if lead_due && !s.fired.contains(&lead_key) {
            s.fired.insert(lead_key);
            let minutes = std::cmp::max(1, (ev.start - now) / 60_000);
            let message = prefs
                .message_template
                .replace("{title}", &ev.title)
                .replace("{minutes}", &minutes.to_string());
            fly_across(
                handle,
                Flight {
                    message,
                    duration_ms: 9000,
                    sound: Some(prefs.sound_enabled),
                    sound_pack: Some(prefs.sound_pack.clone()),
                    theme: Some(prefs.theme.clone()),
                    head: Some(prefs.flier_head.clone()),
                    color: Some(prefs.flier_color.clone()),
                    font: Some(prefs.font.clone()),
                },
            );
        }

        // 2) Optional second fly-by at the start time.
        if prefs.fly_at_start {
            let start_key = format!("{}:start", ev.id);
            let start_due = now >= ev.start && now < ev.start + FIRE_WINDOW_MS;
            if start_due && !s.fired.contains(&start_key) {
                s.fired.insert(start_key);
                fly_across(
                    handle,
                    Flight {
                        message: format!("{} starting now", ev.title),
                        duration_ms: 9000,
                        sound: Some(prefs.sound_enabled),
                        sound_pack: Some(prefs.sound_pack.clone()),
                        theme: Some(prefs.theme.clone()),
                        head: Some(prefs.flier_head.clone()),
                        color: Some(prefs.flier_color.clone()),
                        font: Some(prefs.font.clone()),
                    },
                );
            }
        }
    }
}
