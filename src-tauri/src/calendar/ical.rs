use super::types::{ProviderStatus, UpcomingEvent};
use chrono::Utc;
use icalendar::{Calendar, CalendarComponent, Component, DatePerhapsTime};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

const STORE_FILE: &str = "ical-feeds.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Feed {
    pub id: String,
    pub name: String,
    pub url: String,
}

#[derive(Default)]
pub struct Store {
    feeds: Arc<RwLock<Vec<Feed>>>,
}

fn store_path() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("studio.qruakpit.desktop");
    let _ = fs::create_dir_all(&dir);
    dir.join(STORE_FILE)
}

impl Store {
    pub fn load() -> Self {
        let feeds = fs::read_to_string(store_path())
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<Feed>>(&s).ok())
            .unwrap_or_default();
        Self {
            feeds: Arc::new(RwLock::new(feeds)),
        }
    }

    fn persist(feeds: &[Feed]) {
        if let Ok(s) = serde_json::to_string_pretty(feeds) {
            let _ = fs::write(store_path(), s);
        }
    }

    pub fn status(&self) -> ProviderStatus {
        let feeds = self.feeds.try_read();
        let count = feeds.as_ref().map(|f| f.len()).unwrap_or(0);
        ProviderStatus {
            id: "ical".into(),
            name: "iCal subscription".into(),
            connected: count > 0,
            detail: if count > 0 {
                Some(format!("{} feed{}", count, if count == 1 { "" } else { "s" }))
            } else {
                None
            },
            configured: Some(true),
        }
    }

    pub async fn list(&self) -> Vec<Feed> {
        self.feeds.read().await.clone()
    }

    pub async fn add(&self, url: String, name: Option<String>) -> Result<Vec<Feed>, String> {
        // Validate by fetching once.
        let body = reqwest::get(&url)
            .await
            .map_err(|e| format!("fetch failed: {e}"))?
            .text()
            .await
            .map_err(|e| format!("read failed: {e}"))?;
        let _cal: Calendar = body
            .parse()
            .map_err(|e: String| format!("invalid ICS: {e}"))?;

        let feed = Feed {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.unwrap_or_else(|| nice_name_from_url(&url)),
            url,
        };
        let mut feeds = self.feeds.write().await;
        feeds.push(feed);
        Self::persist(&feeds);
        Ok(feeds.clone())
    }

    pub async fn remove(&self, id: String) -> Vec<Feed> {
        let mut feeds = self.feeds.write().await;
        feeds.retain(|f| f.id != id);
        Self::persist(&feeds);
        feeds.clone()
    }

    pub async fn list_upcoming(&self, minutes: i64) -> Result<Vec<UpcomingEvent>, String> {
        let feeds = self.feeds.read().await.clone();
        let now = Utc::now();
        let window_end = now + chrono::Duration::minutes(minutes);
        let mut out: Vec<UpcomingEvent> = Vec::new();

        for feed in feeds {
            let body = match reqwest::get(&feed.url).await {
                Ok(r) => match r.text().await {
                    Ok(t) => t,
                    Err(_) => continue,
                },
                Err(_) => continue,
            };
            let cal: Calendar = match body.parse() {
                Ok(c) => c,
                Err(_) => continue,
            };

            for comp in cal.components {
                if let CalendarComponent::Event(event) = comp {
                    let start = match event.get_start() {
                        Some(dt) => match dperhaps_to_unix_ms(&dt) {
                            Some(ms) => ms,
                            None => continue,
                        },
                        None => continue,
                    };
                    let end = event
                        .get_end()
                        .and_then(|dt| dperhaps_to_unix_ms(&dt))
                        .unwrap_or(start);

                    let start_dt = chrono::DateTime::<Utc>::from_timestamp_millis(start)
                        .unwrap_or(now);
                    if start_dt < now || start_dt > window_end {
                        continue;
                    }
                    let title = event.get_summary().unwrap_or("(untitled)").to_string();
                    let uid = event.get_uid().unwrap_or("").to_string();
                    let id = format!("ical:{}:{}", feed.id, uid);
                    out.push(UpcomingEvent { id, title, start, end });
                }
            }
        }
        Ok(out)
    }
}

fn nice_name_from_url(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_else(|| "Subscription".into())
}

fn dperhaps_to_unix_ms(d: &DatePerhapsTime) -> Option<i64> {
    match d {
        DatePerhapsTime::DateTime(dt) => match dt {
            icalendar::CalendarDateTime::Utc(dt) => Some(dt.timestamp_millis()),
            icalendar::CalendarDateTime::Floating(naive) => Some(
                chrono::DateTime::<Utc>::from_naive_utc_and_offset(*naive, Utc).timestamp_millis(),
            ),
            icalendar::CalendarDateTime::WithTimezone { date_time, tzid } => {
                let tz: chrono_tz::Tz = tzid.parse().ok()?;
                let dt = date_time.and_local_timezone(tz).single()?;
                Some(dt.with_timezone(&Utc).timestamp_millis())
            }
        },
        DatePerhapsTime::Date(d) => {
            let naive = d.and_hms_opt(0, 0, 0)?;
            Some(chrono::DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).timestamp_millis())
        }
    }
}
