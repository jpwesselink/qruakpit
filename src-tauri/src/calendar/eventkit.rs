use super::types::{ProviderStatus, UpcomingEvent};
use std::sync::Arc;
use tokio::sync::RwLock;

/// User-visible info about one calendar surfaced from EventKit.
#[derive(Debug, Clone)]
pub struct EventKitCalendar {
    pub id: String,
    pub title: String,
    pub source: String,
}

/// EventKit "provider". Reads calendars and events from macOS Calendar.app.
///
/// "Connect" means: request TCC access. "Configure" means: toggle which
/// individual calendars are included in the upcoming-events list. The
/// allow-list lives in prefs (`eventkit_enabled_calendars: Vec<String>`).
#[derive(Default)]
pub struct Provider {
    inner: Arc<RwLock<Inner>>,
}

#[derive(Default)]
struct Inner {
    access: AccessState,
    enabled_calendars: Vec<String>,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessState {
    #[default]
    NotDetermined,
    Granted,
    Denied,
}

impl Provider {
    pub fn restore(enabled_calendars: Vec<String>) -> Self {
        let access = native::current_access_state();
        Self {
            inner: Arc::new(RwLock::new(Inner {
                access,
                enabled_calendars,
            })),
        }
    }

    pub fn status(&self) -> ProviderStatus {
        let inner = self.inner.try_read();
        let (state, count) = match inner.as_ref() {
            Ok(i) => (i.access, i.enabled_calendars.len()),
            Err(_) => (AccessState::NotDetermined, 0),
        };
        let connected = state == AccessState::Granted && count > 0;
        let detail = match (state, count) {
            (AccessState::Granted, 0) => Some("Access granted, no calendars selected".into()),
            (AccessState::Granted, n) => Some(format!("{n} calendar{}", if n == 1 { "" } else { "s" })),
            (AccessState::Denied, _) => Some("Access denied (System Settings -> Privacy)".into()),
            (AccessState::NotDetermined, _) => None,
        };
        ProviderStatus {
            id: "eventkit".into(),
            name: "macOS Calendar (Outlook / Exchange / Google / iCloud)".into(),
            connected,
            detail,
            configured: Some(true),
        }
    }

    pub async fn request_access(&self) -> Result<AccessState, String> {
        let state = native::request_access().await?;
        self.inner.write().await.access = state;
        Ok(state)
    }

    pub async fn list_calendars(&self) -> Result<Vec<EventKitCalendar>, String> {
        // Trigger the prompt on the first call if not yet determined.
        let access = self.inner.read().await.access;
        let access = if access == AccessState::NotDetermined {
            self.request_access().await?
        } else {
            access
        };
        if access != AccessState::Granted {
            return Err("Calendar access not granted".into());
        }
        native::list_calendars()
    }

    pub async fn set_enabled(&self, ids: Vec<String>) {
        self.inner.write().await.enabled_calendars = ids;
    }

    pub async fn enabled(&self) -> Vec<String> {
        self.inner.read().await.enabled_calendars.clone()
    }

    pub async fn list_upcoming(&self, minutes: i64) -> Result<Vec<UpcomingEvent>, String> {
        let inner = self.inner.read().await;
        if inner.access != AccessState::Granted || inner.enabled_calendars.is_empty() {
            return Ok(Vec::new());
        }
        let ids = inner.enabled_calendars.clone();
        drop(inner);
        native::list_upcoming(&ids, minutes)
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::{AccessState, EventKitCalendar};
    use crate::calendar::types::UpcomingEvent;
    use cocoa::base::{id, nil, BOOL, NO};
    use objc::runtime::Sel;
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::CStr;
    use std::sync::Mutex;
    use tokio::sync::oneshot;

    const EK_ENTITY_TYPE_EVENT: u64 = 0;
    const EK_AUTH_STATUS_NOT_DETERMINED: i64 = 0;
    const EK_AUTH_STATUS_RESTRICTED: i64 = 1;
    const EK_AUTH_STATUS_DENIED: i64 = 2;
    const EK_AUTH_STATUS_AUTHORIZED: i64 = 3;
    const EK_AUTH_STATUS_WRITE_ONLY: i64 = 4;
    const EK_AUTH_STATUS_FULL_ACCESS: i64 = 5;

    fn ns_string_to_rust(s: id) -> String {
        if s.is_null() {
            return String::new();
        }
        unsafe {
            let utf8: *const i8 = msg_send![s, UTF8String];
            if utf8.is_null() {
                return String::new();
            }
            CStr::from_ptr(utf8).to_string_lossy().into_owned()
        }
    }

    pub fn current_access_state() -> AccessState {
        unsafe {
            let cls = class!(EKEventStore);
            let status: i64 = msg_send![cls, authorizationStatusForEntityType: EK_ENTITY_TYPE_EVENT];
            match status {
                EK_AUTH_STATUS_AUTHORIZED | EK_AUTH_STATUS_FULL_ACCESS => AccessState::Granted,
                EK_AUTH_STATUS_DENIED | EK_AUTH_STATUS_RESTRICTED => AccessState::Denied,
                EK_AUTH_STATUS_WRITE_ONLY => AccessState::Denied,
                EK_AUTH_STATUS_NOT_DETERMINED => AccessState::NotDetermined,
                _ => AccessState::NotDetermined,
            }
        }
    }

    /// Requests Calendar access asynchronously. Triggers the TCC prompt on first call.
    pub async fn request_access() -> Result<AccessState, String> {
        // Hold the EKEventStore alive across the async wait. The completion block
        // calls back on an arbitrary queue; we hop back to the awaiter via oneshot.
        let (tx, rx) = oneshot::channel::<bool>();
        let tx = Mutex::new(Some(tx));

        unsafe {
            let store: id = msg_send![class!(EKEventStore), new];
            if store.is_null() {
                return Err("Failed to create EKEventStore".into());
            }

            // Build the completion block.
            let block = block::ConcreteBlock::new(move |granted: BOOL, _err: id| {
                let granted = granted != NO;
                if let Ok(mut guard) = tx.lock() {
                    if let Some(sender) = guard.take() {
                        let _ = sender.send(granted);
                    }
                }
            });
            let block = block.copy();

            // macOS 14+: requestFullAccessToEventsWithCompletion:
            // macOS 11-13: requestAccessToEntityType:completion:
            let sel_full: Sel = sel!(requestFullAccessToEventsWithCompletion:);
            let responds_to_full: BOOL = msg_send![store, respondsToSelector: sel_full];
            if responds_to_full != NO {
                let _: () = msg_send![store, requestFullAccessToEventsWithCompletion: &*block];
            } else {
                let _: () = msg_send![store, requestAccessToEntityType: EK_ENTITY_TYPE_EVENT completion: &*block];
            }
            // Intentional retention of the store pointer; the completion block needs it alive.
            // `store` is a raw `*mut Object`, Copy, so let-bind to prevent the value being dropped early.
            let _kept_alive = store;
        }

        match rx.await {
            Ok(true) => Ok(AccessState::Granted),
            Ok(false) => Ok(AccessState::Denied),
            Err(_) => Err("Calendar access request cancelled".into()),
        }
    }

    pub fn list_calendars() -> Result<Vec<EventKitCalendar>, String> {
        unsafe {
            let store: id = msg_send![class!(EKEventStore), new];
            if store.is_null() {
                return Err("Failed to create EKEventStore".into());
            }
            let calendars: id = msg_send![store, calendarsForEntityType: EK_ENTITY_TYPE_EVENT];
            if calendars.is_null() {
                return Ok(Vec::new());
            }
            let count: usize = msg_send![calendars, count];
            let mut out = Vec::with_capacity(count);
            for i in 0..count {
                let cal: id = msg_send![calendars, objectAtIndex: i];
                if cal.is_null() {
                    continue;
                }
                let identifier: id = msg_send![cal, calendarIdentifier];
                let title: id = msg_send![cal, title];
                let source: id = msg_send![cal, source];
                let source_title: id = if source.is_null() {
                    nil
                } else {
                    msg_send![source, title]
                };
                out.push(EventKitCalendar {
                    id: ns_string_to_rust(identifier),
                    title: ns_string_to_rust(title),
                    source: ns_string_to_rust(source_title),
                });
            }
            let _: () = msg_send![store, release];
            Ok(out)
        }
    }

    pub fn list_upcoming(ids: &[String], minutes: i64) -> Result<Vec<UpcomingEvent>, String> {
        unsafe {
            let store: id = msg_send![class!(EKEventStore), new];
            if store.is_null() {
                return Err("Failed to create EKEventStore".into());
            }

            // Resolve EKCalendar objects by identifier.
            let all: id = msg_send![store, calendarsForEntityType: EK_ENTITY_TYPE_EVENT];
            if all.is_null() {
                return Ok(Vec::new());
            }
            let count: usize = msg_send![all, count];
            let wanted: std::collections::HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
            let nsmut: id = msg_send![class!(NSMutableArray), array];
            for i in 0..count {
                let cal: id = msg_send![all, objectAtIndex: i];
                if cal.is_null() {
                    continue;
                }
                let identifier: id = msg_send![cal, calendarIdentifier];
                let id_str = ns_string_to_rust(identifier);
                if wanted.contains(id_str.as_str()) {
                    let _: () = msg_send![nsmut, addObject: cal];
                }
            }

            // Build the time window predicate.
            let now: id = msg_send![class!(NSDate), date];
            let end: id = msg_send![now, dateByAddingTimeInterval: (minutes as f64) * 60.0];
            let predicate: id =
                msg_send![store, predicateForEventsWithStartDate: now endDate: end calendars: nsmut];

            // Run the query (synchronous variant).
            let events: id = msg_send![store, eventsMatchingPredicate: predicate];
            if events.is_null() {
                let _: () = msg_send![store, release];
                return Ok(Vec::new());
            }
            let n: usize = msg_send![events, count];
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let ev: id = msg_send![events, objectAtIndex: i];
                if ev.is_null() {
                    continue;
                }
                let identifier: id = msg_send![ev, eventIdentifier];
                let title: id = msg_send![ev, title];
                let start_date: id = msg_send![ev, startDate];
                let end_date: id = msg_send![ev, endDate];

                let start_secs: f64 = if start_date.is_null() {
                    0.0
                } else {
                    msg_send![start_date, timeIntervalSince1970]
                };
                let end_secs: f64 = if end_date.is_null() {
                    start_secs
                } else {
                    msg_send![end_date, timeIntervalSince1970]
                };

                out.push(UpcomingEvent {
                    id: ns_string_to_rust(identifier),
                    title: ns_string_to_rust(title),
                    start: (start_secs * 1000.0) as i64,
                    end: (end_secs * 1000.0) as i64,
                });
            }
            let _: () = msg_send![store, release];
            Ok(out)
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    use super::{AccessState, EventKitCalendar};
    use crate::calendar::types::UpcomingEvent;

    pub fn current_access_state() -> AccessState {
        AccessState::Denied
    }

    pub async fn request_access() -> Result<AccessState, String> {
        Err("EventKit is only available on macOS".into())
    }

    pub fn list_calendars() -> Result<Vec<EventKitCalendar>, String> {
        Ok(Vec::new())
    }

    pub fn list_upcoming(_ids: &[String], _minutes: i64) -> Result<Vec<UpcomingEvent>, String> {
        Ok(Vec::new())
    }
}
