pub mod eventkit;
pub mod google;
pub mod ical;
pub mod icloud;
pub mod oauth_loopback;
pub mod types;

use crate::store::Prefs;
pub use types::{ProviderStatus, UpcomingEvent};

/// In-memory state for all calendar providers.
#[derive(Default)]
pub struct State {
    pub google: google::Session,
    pub icloud: icloud::Session,
    pub eventkit: eventkit::Provider,
    pub ical: ical::Store,
}

impl State {
    /// Restore any saved provider sessions (best-effort, no UI prompts).
    pub fn load(prefs: &Prefs) -> Self {
        Self {
            google: google::Session::restore(),
            icloud: icloud::Session::restore(),
            eventkit: eventkit::Provider::restore(prefs.eventkit_enabled_calendars.clone()),
            ical: ical::Store::load(),
        }
    }

    pub fn statuses(&self) -> Vec<ProviderStatus> {
        vec![
            self.ical.status(),
            self.google.status(),
            self.icloud.status(),
            self.eventkit.status(),
        ]
    }

    /// Merged, deduplicated, sorted upcoming events across all providers.
    /// Deduplication: first by provider-prefixed id (exact same source), then by
    /// (title, start) so the same meeting present in EventKit and Google does
    /// not fire two flights.
    pub async fn list_upcoming(&self, minutes: i64) -> Vec<UpcomingEvent> {
        let (g, c, e, i) = tokio::join!(
            self.google.list_upcoming(minutes),
            self.icloud.list_upcoming(minutes),
            self.eventkit.list_upcoming(minutes),
            self.ical.list_upcoming(minutes),
        );

        let mut all: Vec<UpcomingEvent> = Vec::new();
        all.extend(g.unwrap_or_default().into_iter().map(|mut e| {
            e.id = format!("google:{}", e.id);
            e
        }));
        all.extend(c.unwrap_or_default().into_iter().map(|mut e| {
            e.id = format!("icloud:{}", e.id);
            e
        }));
        all.extend(e.unwrap_or_default().into_iter().map(|mut e| {
            e.id = format!("eventkit:{}", e.id);
            e
        }));
        all.extend(i.unwrap_or_default()); // already prefixed with ical:<feed>:
        all.sort_by_key(|e| e.start);

        let mut seen_id = std::collections::HashSet::new();
        let mut seen_pair = std::collections::HashSet::new();
        all.retain(|ev| {
            if !seen_id.insert(ev.id.clone()) {
                return false;
            }
            let pair = (ev.title.clone(), ev.start);
            seen_pair.insert(pair)
        });
        all
    }
}
