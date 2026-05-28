use super::oauth_loopback;
use super::types::{ProviderStatus, UpcomingEvent};
use crate::store::{keychain_delete, keychain_get, keychain_set};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

const CREDS_KEY: &str = "google.creds";
const TOKEN_KEY: &str = "google.refresh_token";
const EMAIL_KEY: &str = "google.email";

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/calendar.readonly openid email";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Creds {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Default)]
pub struct Session {
    inner: Arc<RwLock<Inner>>,
}

#[derive(Default)]
struct Inner {
    creds: Option<Creds>,
    refresh_token: Option<String>,
    email: Option<String>,
}

impl Session {
    pub fn restore() -> Self {
        let creds = keychain_get(CREDS_KEY).and_then(|s| serde_json::from_str::<Creds>(&s).ok());
        let refresh_token = keychain_get(TOKEN_KEY);
        let email = keychain_get(EMAIL_KEY);
        Self {
            inner: Arc::new(RwLock::new(Inner {
                creds,
                refresh_token,
                email,
            })),
        }
    }

    pub fn status(&self) -> ProviderStatus {
        let inner = self.inner.try_read();
        let (configured, connected, detail) = match inner.as_ref() {
            Ok(i) => (i.creds.is_some(), i.refresh_token.is_some(), i.email.clone()),
            Err(_) => (false, false, None),
        };
        ProviderStatus {
            id: "google".into(),
            name: "Google Calendar".into(),
            connected,
            detail,
            configured: Some(configured),
        }
    }

    pub async fn set_creds(&self, client_id: String, client_secret: String) {
        let creds = Creds {
            client_id,
            client_secret,
        };
        let _ = keychain_set(CREDS_KEY, &serde_json::to_string(&creds).unwrap_or_default());
        self.inner.write().await.creds = Some(creds);
    }

    pub async fn disconnect(&self) {
        keychain_delete(TOKEN_KEY);
        keychain_delete(EMAIL_KEY);
        let mut i = self.inner.write().await;
        i.refresh_token = None;
        i.email = None;
    }

    pub async fn connect(&self) -> Result<(), String> {
        let creds = self
            .inner
            .read()
            .await
            .creds
            .clone()
            .ok_or_else(|| "Set client id + secret first".to_string())?;

        // Spin up the loopback redirect server.
        let server = oauth_loopback::start(Duration::from_secs(180)).await?;
        let redirect = server.redirect_uri.clone();

        // Build PKCE pair (S256).
        let verifier = pkce_verifier();
        let challenge = pkce_challenge(&verifier);
        let state = random_state();

        let auth_url = format!(
            "{AUTH_URL}?response_type=code&access_type=offline&prompt=consent&include_granted_scopes=true&client_id={cid}&redirect_uri={ru}&scope={scope}&state={st}&code_challenge={ch}&code_challenge_method=S256",
            cid = url_encode(&creds.client_id),
            ru = url_encode(&redirect),
            scope = url_encode(SCOPE),
            st = url_encode(&state),
            ch = url_encode(&challenge),
        );

        // Open the system browser. tauri-plugin-shell would be cleaner but
        // requires an AppHandle; the `open` crate is already pulled in by Tauri.
        if let Err(e) = open::that(&auth_url) {
            return Err(format!("Failed to open browser: {e}"));
        }

        // Wait for the loopback callback.
        let cb = server.wait_for_callback.await?;
        if let Some(err) = cb.error {
            return Err(format!("OAuth error: {err}"));
        }
        if cb.state.as_deref() != Some(state.as_str()) {
            return Err("OAuth state mismatch".into());
        }
        let code = cb
            .code
            .ok_or_else(|| "OAuth callback missing code".to_string())?;

        // Exchange the authorization code for tokens.
        let client = reqwest::Client::new();
        let token_resp: TokenResponse = client
            .post(TOKEN_URL)
            .form(&[
                ("client_id", creds.client_id.as_str()),
                ("client_secret", creds.client_secret.as_str()),
                ("code", code.as_str()),
                ("code_verifier", verifier.as_str()),
                ("grant_type", "authorization_code"),
                ("redirect_uri", redirect.as_str()),
            ])
            .send()
            .await
            .map_err(|e| format!("token request: {e}"))?
            .error_for_status()
            .map_err(|e| format!("token http error: {e}"))?
            .json()
            .await
            .map_err(|e| format!("token decode: {e}"))?;

        let refresh = token_resp
            .refresh_token
            .ok_or_else(|| "Google did not return a refresh_token".to_string())?;
        let _ = keychain_set(TOKEN_KEY, &refresh);

        let mut email: Option<String> = None;
        if let Some(access) = token_resp.access_token.as_deref() {
            if let Ok(profile) = client
                .get("https://openidconnect.googleapis.com/v1/userinfo")
                .bearer_auth(access)
                .send()
                .await
            {
                if let Ok(p) = profile.json::<UserInfo>().await {
                    email = p.email;
                }
            }
        }
        if let Some(e) = &email {
            let _ = keychain_set(EMAIL_KEY, e);
        }

        let mut i = self.inner.write().await;
        i.refresh_token = Some(refresh);
        i.email = email;
        Ok(())
    }

    pub async fn list_upcoming(&self, minutes: i64) -> Result<Vec<UpcomingEvent>, String> {
        let (creds, refresh) = {
            let i = self.inner.read().await;
            (i.creds.clone(), i.refresh_token.clone())
        };
        let (Some(creds), Some(refresh)) = (creds, refresh) else {
            return Ok(Vec::new());
        };

        let access = refresh_access_token(&creds, &refresh).await?;
        let now = Utc::now();
        let end = now + chrono::Duration::minutes(minutes);

        let client = reqwest::Client::new();
        // Primary calendar is sufficient for the meeting-reminder case.
        let url = format!(
            "https://www.googleapis.com/calendar/v3/calendars/primary/events?timeMin={start}&timeMax={end}&singleEvents=true&orderBy=startTime&maxResults=50",
            start = url_encode(&now.to_rfc3339()),
            end = url_encode(&end.to_rfc3339()),
        );
        let resp: EventsResponse = client
            .get(&url)
            .bearer_auth(&access)
            .send()
            .await
            .map_err(|e| format!("events request: {e}"))?
            .error_for_status()
            .map_err(|e| format!("events http error: {e}"))?
            .json()
            .await
            .map_err(|e| format!("events decode: {e}"))?;

        Ok(resp
            .items
            .into_iter()
            .filter_map(|ev| {
                let start = parse_event_time(ev.start.as_ref()?)?;
                let end = ev
                    .end
                    .as_ref()
                    .and_then(parse_event_time)
                    .unwrap_or(start);
                Some(UpcomingEvent {
                    id: ev.id,
                    title: ev.summary.unwrap_or_else(|| "(no title)".into()),
                    start,
                    end,
                })
            })
            .collect())
    }
}

async fn refresh_access_token(creds: &Creds, refresh: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp: TokenResponse = client
        .post(TOKEN_URL)
        .form(&[
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
        ])
        .send()
        .await
        .map_err(|e| format!("refresh request: {e}"))?
        .error_for_status()
        .map_err(|e| format!("refresh http error: {e}"))?
        .json()
        .await
        .map_err(|e| format!("refresh decode: {e}"))?;
    resp.access_token
        .ok_or_else(|| "refresh response missing access_token".into())
}

// --- Helpers --------------------------------------------------------------

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn pkce_verifier() -> String {
    use base64::Engine;
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn pkce_challenge(verifier: &str) -> String {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let digest = hasher.finalize();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

fn random_state() -> String {
    use base64::Engine;
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

// --- Wire types -----------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EventsResponse {
    #[serde(default)]
    items: Vec<RawEvent>,
}

#[derive(Debug, Deserialize)]
struct RawEvent {
    id: String,
    summary: Option<String>,
    start: Option<RawTime>,
    end: Option<RawTime>,
}

#[derive(Debug, Deserialize)]
struct RawTime {
    #[serde(default, rename = "dateTime")]
    date_time: Option<String>,
    #[serde(default)]
    date: Option<String>,
}

fn parse_event_time(t: &RawTime) -> Option<i64> {
    if let Some(dt) = &t.date_time {
        return DateTime::parse_from_rfc3339(dt)
            .ok()
            .map(|d| d.timestamp_millis());
    }
    if let Some(d) = &t.date {
        // All-day events: midnight UTC of that date.
        let naive = chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .ok()?
            .and_hms_opt(0, 0, 0)?;
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc).timestamp_millis());
    }
    None
}
