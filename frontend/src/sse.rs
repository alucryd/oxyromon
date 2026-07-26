//! Server-Sent Events client and notification helpers (ports `src/events.js`).

use std::sync::atomic::{AtomicU64, Ordering};

use gloo_timers::callback::Timeout;
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{EventSource, MessageEvent};

use crate::api::get_systems;
use crate::model::{Notification, NotificationKind};
use crate::state::AppState;

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// Endpoint used by the Import DAT modal to POST multipart uploads.
pub const DATS_ENDPOINT: &str = "/dats";

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
    state.notifications.update(|list| {
        list.insert(0, notification.clone());
    });
    state.toast.set(Some(notification));

    // Auto-dismiss the toast (info: 3s, otherwise 5s), matching the Svelte UI.
    let duration = if kind == NotificationKind::Info { 3000 } else { 5000 };
    Timeout::new(duration, move || state.toast.set(None)).forget();
}

fn message_field(data: &Value) -> String {
    data["message"].as_str().unwrap_or_default().to_string()
}

/// Register an SSE listener that pushes a notification of a fixed kind.
fn on_event(source: &EventSource, name: &'static str, state: AppState, kind: NotificationKind) {
    let handler = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let data: Value = event
            .data()
            .as_string()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        push_notification(state, message_field(&data), kind);
    });
    source
        .add_event_listener_with_callback(name, handler.as_ref().unchecked_ref())
        .ok();
    handler.forget();
}

/// Register a "completed" listener that also refreshes the systems list.
fn on_complete_event(
    source: &EventSource,
    name: &'static str,
    state: AppState,
    success_kind: NotificationKind,
) {
    let handler = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let data: Value = event
            .data()
            .as_string()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        let skipped = data["skipped"].as_bool().unwrap_or(false);
        let kind = if skipped {
            NotificationKind::Warning
        } else {
            success_kind
        };
        push_notification(state, message_field(&data), kind);
        if !skipped {
            spawn_local(async move { get_systems(state).await });
        }
    });
    source
        .add_event_listener_with_callback(name, handler.as_ref().unchecked_ref())
        .ok();
    handler.forget();
}

/// Open the SSE connection and wire up all listeners.
///
/// The connection lives for the lifetime of the SPA, so closures are
/// intentionally leaked with `forget()` rather than tracked for teardown.
pub fn connect_sse(state: AppState) {
    let source = match EventSource::new("/events") {
        Ok(source) => source,
        Err(e) => {
            leptos::logging::error!("SSE connect failed: {e:?}");
            return;
        }
    };

    on_event(&source, "purge_started", state, NotificationKind::Info);
    on_complete_event(&source, "purge_complete", state, NotificationKind::Success);
    on_event(&source, "purge_error", state, NotificationKind::Error);
    on_event(&source, "import_dat_started", state, NotificationKind::Info);
    on_complete_event(
        &source,
        "import_dat_complete",
        state,
        NotificationKind::Success,
    );
    on_event(&source, "import_dat_error", state, NotificationKind::Error);

    // Keep the EventSource alive for the app lifetime.
    std::mem::forget(source);
}
