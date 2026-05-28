use super::types::{ProviderStatus, UpcomingEvent};
use crate::store::{keychain_delete, keychain_get, keychain_set};
use base64::Engine;
use chrono::{Duration as ChronoDuration, Utc};
use icalendar::{Calendar as IcalCalendar, CalendarComponent, Component, DatePerhapsTime};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

const USER_KEY: &str = "icloud.username";
const PASS_KEY: &str = "icloud.password";

const WELL_KNOWN: &str = "https://caldav.icloud.com/.well-known/caldav";

#[derive(Default)]
pub struct Session {
    inner: Arc<RwLock<Inner>>,
}

#[derive(Default)]
struct Inner {
    username: Option<String>,
    password: Option<String>,
}

impl Session {
    pub fn restore() -> Self {
        let username = keychain_get(USER_KEY);
        let password = keychain_get(PASS_KEY);
        Self {
            inner: Arc::new(RwLock::new(Inner { username, password })),
        }
    }

    pub fn status(&self) -> ProviderStatus {
        let i = self.inner.try_read();
        let (connected, detail) = match i.as_ref() {
            Ok(i) => (i.password.is_some(), i.username.clone()),
            Err(_) => (false, None),
        };
        ProviderStatus {
            id: "icloud".into(),
            name: "iCloud".into(),
            connected,
            detail,
            configured: Some(true),
        }
    }

    pub async fn connect(&self, username: String, password: String) -> Result<(), String> {
        // Probe so we fail fast on bad credentials.
        let probe = caldav_request(
            &username,
            &password,
            WELL_KNOWN,
            "PROPFIND",
            "0",
            r#"<?xml version="1.0" encoding="utf-8" ?><propfind xmlns="DAV:"><prop><current-user-principal/></prop></propfind>"#,
        )
        .await?;
        if !probe.status.is_success() {
            return Err(format!("iCloud login failed (HTTP {})", probe.status.as_u16()));
        }
        let _ = keychain_set(USER_KEY, &username);
        let _ = keychain_set(PASS_KEY, &password);
        let mut i = self.inner.write().await;
        i.username = Some(username);
        i.password = Some(password);
        Ok(())
    }

    pub async fn disconnect(&self) {
        keychain_delete(USER_KEY);
        keychain_delete(PASS_KEY);
        let mut i = self.inner.write().await;
        i.username = None;
        i.password = None;
    }

    pub async fn list_upcoming(&self, minutes: i64) -> Result<Vec<UpcomingEvent>, String> {
        let (user, pass) = {
            let i = self.inner.read().await;
            (i.username.clone(), i.password.clone())
        };
        let (Some(user), Some(pass)) = (user, pass) else {
            return Ok(Vec::new());
        };

        // 1) PROPFIND well-known to discover the user's principal URL.
        let principal_url = discover_principal(&user, &pass).await?;

        // 2) PROPFIND the principal to find calendar-home-set.
        let home = discover_home_set(&user, &pass, &principal_url).await?;

        // 3) PROPFIND the home to enumerate calendars (only VEVENT collections).
        let calendars = enumerate_calendars(&user, &pass, &home).await?;

        // 4) REPORT each calendar with a time-range filter.
        let now = Utc::now();
        let end = now + ChronoDuration::minutes(minutes);
        let start_z = now.format("%Y%m%dT%H%M%SZ").to_string();
        let end_z = end.format("%Y%m%dT%H%M%SZ").to_string();

        let mut out: Vec<UpcomingEvent> = Vec::new();
        for cal_url in calendars {
            let body = format!(
                r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:getetag/>
    <c:calendar-data/>
  </d:prop>
  <c:filter>
    <c:comp-filter name="VCALENDAR">
      <c:comp-filter name="VEVENT">
        <c:time-range start="{start_z}" end="{end_z}"/>
      </c:comp-filter>
    </c:comp-filter>
  </c:filter>
</c:calendar-query>"#
            );
            let resp = match caldav_request(&user, &pass, &cal_url, "REPORT", "1", &body).await {
                Ok(r) => r,
                Err(_) => continue,
            };
            if !resp.status.is_success() {
                continue;
            }
            for ics in extract_calendar_data(&resp.body) {
                if let Ok(cal) = ics.parse::<IcalCalendar>() {
                    for comp in cal.components {
                        if let CalendarComponent::Event(ev) = comp {
                            let Some(start_dt) = ev.get_start().and_then(|d| dt_to_unix_ms(&d))
                            else {
                                continue;
                            };
                            let end_dt = ev
                                .get_end()
                                .and_then(|d| dt_to_unix_ms(&d))
                                .unwrap_or(start_dt);
                            let title =
                                ev.get_summary().unwrap_or("(untitled)").to_string();
                            let uid = ev.get_uid().unwrap_or("").to_string();
                            out.push(UpcomingEvent {
                                id: uid,
                                title,
                                start: start_dt,
                                end: end_dt,
                            });
                        }
                    }
                }
            }
        }

        Ok(out)
    }
}

struct CaldavResponse {
    status: reqwest::StatusCode,
    body: String,
}

async fn caldav_request(
    user: &str,
    pass: &str,
    url: &str,
    method: &str,
    depth: &str,
    body: &str,
) -> Result<CaldavResponse, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| e.to_string())?;
    let auth = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"))
    );
    let resp = client
        .request(method.parse().map_err(|_| "bad HTTP method")?, url)
        .header("Authorization", auth)
        .header("Depth", depth)
        .header("Content-Type", "application/xml; charset=utf-8")
        .body(body.to_string())
        .send()
        .await
        .map_err(|e| format!("caldav request: {e}"))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    Ok(CaldavResponse { status, body })
}

async fn discover_principal(user: &str, pass: &str) -> Result<String, String> {
    let resp = caldav_request(
        user,
        pass,
        WELL_KNOWN,
        "PROPFIND",
        "0",
        r#"<?xml version="1.0" encoding="utf-8" ?><propfind xmlns="DAV:"><prop><current-user-principal/></prop></propfind>"#,
    )
    .await?;
    if !resp.status.is_success() {
        return Err(format!("principal discovery: HTTP {}", resp.status));
    }
    extract_href(&resp.body, "current-user-principal")
        .or_else(|| extract_href(&resp.body, "href"))
        .map(|href| absolutize("https://caldav.icloud.com", &href))
        .ok_or_else(|| "principal href not found".into())
}

async fn discover_home_set(user: &str, pass: &str, principal: &str) -> Result<String, String> {
    let resp = caldav_request(
        user,
        pass,
        principal,
        "PROPFIND",
        "0",
        r#"<?xml version="1.0" encoding="utf-8" ?><propfind xmlns="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><prop><c:calendar-home-set/></prop></propfind>"#,
    )
    .await?;
    if !resp.status.is_success() {
        return Err(format!("home-set discovery: HTTP {}", resp.status));
    }
    extract_href(&resp.body, "calendar-home-set")
        .or_else(|| extract_href(&resp.body, "href"))
        .map(|href| absolutize(principal, &href))
        .ok_or_else(|| "home-set href not found".into())
}

async fn enumerate_calendars(user: &str, pass: &str, home: &str) -> Result<Vec<String>, String> {
    let resp = caldav_request(
        user,
        pass,
        home,
        "PROPFIND",
        "1",
        r#"<?xml version="1.0" encoding="utf-8" ?><propfind xmlns="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><prop><resourcetype/><c:supported-calendar-component-set/></prop></propfind>"#,
    )
    .await?;
    if !resp.status.is_success() {
        return Err(format!("calendar enum: HTTP {}", resp.status));
    }

    let mut out = Vec::new();
    for href in extract_all_hrefs(&resp.body) {
        // We only want collections that report calendar resourcetype and VEVENT support.
        // A cheap heuristic: skip URLs that don't end with '/'.
        let abs = absolutize(home, &href);
        if abs == home {
            continue;
        }
        if !abs.ends_with('/') {
            continue;
        }
        out.push(abs);
    }
    Ok(out)
}

fn extract_href(body: &str, near_tag: &str) -> Option<String> {
    // Very small XML scrape: find `<...:near_tag>` and the next `<href>...</href>` after it.
    // Robust enough for Apple's responses without pulling in a full XML parser dep.
    let idx = body.find(near_tag)?;
    let after = &body[idx..];
    let h_start = after.find("<href")?;
    let inner = &after[h_start..];
    let close = inner.find('>')?;
    let body_start = close + 1;
    let end_idx = inner[body_start..].find("</")?;
    Some(inner[body_start..body_start + end_idx].trim().to_string())
}

fn extract_all_hrefs(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = body[cursor..].find("<href") {
        let abs = cursor + rel;
        let after = &body[abs..];
        let Some(close) = after.find('>') else {
            break;
        };
        let body_start = abs + close + 1;
        let Some(end) = body[body_start..].find("</") else {
            break;
        };
        out.push(body[body_start..body_start + end].trim().to_string());
        cursor = body_start + end;
    }
    out
}

fn extract_calendar_data(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = body[cursor..].find("calendar-data") {
        let abs = cursor + rel;
        let after = &body[abs..];
        let Some(close) = after.find('>') else {
            break;
        };
        let body_start = abs + close + 1;
        let Some(end) = body[body_start..].find("</") else {
            break;
        };
        // Unescape the bare minimum that PROPFIND/REPORT bodies use.
        let raw = body[body_start..body_start + end]
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
            .replace("&#13;", "\r");
        out.push(raw);
        cursor = body_start + end;
    }
    out
}

fn absolutize(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    if let Ok(b) = url::Url::parse(base) {
        if let Ok(joined) = b.join(href) {
            return joined.to_string();
        }
    }
    href.to_string()
}

fn dt_to_unix_ms(d: &DatePerhapsTime) -> Option<i64> {
    use chrono::DateTime;
    match d {
        DatePerhapsTime::DateTime(dt) => match dt {
            icalendar::CalendarDateTime::Utc(dt) => Some(dt.timestamp_millis()),
            icalendar::CalendarDateTime::Floating(naive) => Some(
                DateTime::<Utc>::from_naive_utc_and_offset(*naive, Utc).timestamp_millis(),
            ),
            icalendar::CalendarDateTime::WithTimezone { date_time, tzid } => {
                let tz: chrono_tz::Tz = tzid.parse().ok()?;
                Some(
                    date_time
                        .and_local_timezone(tz)
                        .single()?
                        .with_timezone(&Utc)
                        .timestamp_millis(),
                )
            }
        },
        DatePerhapsTime::Date(d) => {
            let naive = d.and_hms_opt(0, 0, 0)?;
            Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).timestamp_millis())
        }
    }
}
