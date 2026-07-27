//! User-facing notifications: the bell dropdown history plus a transient toast.

use std::sync::atomic::{AtomicU64, Ordering};

use gloo_timers::callback::Timeout;
use leptos::prelude::*;

use crate::model::{Notification, NotificationKind};
use crate::state::AppState;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn now_time() -> String {
    js_sys::Date::new_0()
        .to_locale_time_string("en-US")
        .as_string()
        .unwrap_or_default()
}

/// Prepend a notification and surface it as a transient toast.
pub fn push_notification(state: AppState, message: String, kind: NotificationKind) {
    let notification = Notification {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        message,
        kind,
        time: now_time(),
    };
    let id = notification.id;
    state.notifications.update(|list| {
        list.insert(0, notification.clone());
    });
    state.toast.set(Some(notification));

    // Auto-dismiss (info: 3s, otherwise 5s), matching the Svelte UI. Only clear
    // the toast if it is still this notification, so a later one that replaced
    // it does not get cut short by this timer.
    let duration = if kind == NotificationKind::Info {
        3000
    } else {
        5000
    };
    Timeout::new(duration, move || {
        if state.toast.get_untracked().map(|toast| toast.id) == Some(id) {
            state.toast.set(None);
        }
    })
    .forget();
}
