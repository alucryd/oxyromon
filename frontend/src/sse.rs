//! Server-Sent Events client and notification helpers (ports `src/events.js`).

use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{EventSource, MessageEvent};

use leptos::prelude::*;

use crate::model::NotificationKind;
use crate::notify::push_notification;
use crate::state::AppState;

/// Endpoint used by the Import DAT modal to POST multipart uploads.
pub const DATS_ENDPOINT: &str = "/dats";

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
        push_notification(state.notifier, message_field(&data), kind);
    });
    source
        .add_event_listener_with_callback(name, handler.as_ref().unchecked_ref())
        .ok();
    handler.forget();
}

/// Register a "completed" listener that also refreshes what is on screen.
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
        push_notification(state.notifier, message_field(&data), kind);
        if !skipped {
            // A purged system cannot stay selected: there is nothing left of it
            // to fetch.
            if data["system_id"].as_i64() == Some(state.system_id.get_untracked()) {
                state.system_id.set(-1);
            }
            state.refresh();
        }
    });
    source
        .add_event_listener_with_callback(name, handler.as_ref().unchecked_ref())
        .ok();
    handler.forget();
}

/// Register a "completed" listener that notifies and refreshes what is on
/// screen, since the action changed the selected system's files or completion
/// underneath us.
fn on_refresh(source: &EventSource, name: &'static str, state: AppState) {
    let handler = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let data: Value = event
            .data()
            .as_string()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        push_notification(state.notifier, message_field(&data), NotificationKind::Success);
        state.refresh();
    });
    source
        .add_event_listener_with_callback(name, handler.as_ref().unchecked_ref())
        .ok();
    handler.forget();
}

/// Register listeners that clear an action's busy flag once the server reports
/// it finished, whichever way.
fn on_finished(source: &EventSource, prefix: &str, clear: impl Fn() + Clone + 'static) {
    for outcome in ["complete", "error"] {
        let clear = clear.clone();
        let handler = Closure::<dyn FnMut(MessageEvent)>::new(move |_: MessageEvent| clear());
        source
            .add_event_listener_with_callback(
                &format!("{prefix}_{outcome}"),
                handler.as_ref().unchecked_ref(),
            )
            .ok();
        handler.forget();
    }
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
    on_event(&source, "import_rom_started", state, NotificationKind::Info);
    on_complete_event(
        &source,
        "import_rom_complete",
        state,
        NotificationKind::Success,
    );
    on_event(&source, "import_rom_error", state, NotificationKind::Error);
    on_event(
        &source,
        "download_dats_started",
        state,
        NotificationKind::Info,
    );
    on_complete_event(
        &source,
        "download_dats_complete",
        state,
        NotificationKind::Success,
    );
    on_event(
        &source,
        "download_dats_error",
        state,
        NotificationKind::Error,
    );
    on_event(&source, "sort_roms_started", state, NotificationKind::Info);
    on_event(&source, "sort_roms_error", state, NotificationKind::Error);

    on_refresh(&source, "sort_roms_complete", state);

    on_event(&source, "check_roms_started", state, NotificationKind::Info);
    on_event(&source, "check_roms_error", state, NotificationKind::Error);
    on_refresh(&source, "check_roms_complete", state);

    on_event(&source, "purge_roms_started", state, NotificationKind::Info);
    on_event(&source, "purge_roms_error", state, NotificationKind::Error);
    on_refresh(&source, "purge_roms_complete", state);

    on_event(&source, "convert_roms_started", state, NotificationKind::Info);
    on_event(&source, "convert_roms_error", state, NotificationKind::Error);
    on_refresh(&source, "convert_roms_complete", state);

    on_event(&source, "import_patch_started", state, NotificationKind::Info);
    on_event(&source, "import_patch_error", state, NotificationKind::Error);
    on_refresh(&source, "import_patch_complete", state);

    on_event(&source, "import_irds_started", state, NotificationKind::Info);
    on_event(&source, "import_irds_error", state, NotificationKind::Error);
    on_refresh(&source, "import_irds_complete", state);

    on_event(&source, "purge_irds_started", state, NotificationKind::Info);
    on_event(&source, "purge_irds_error", state, NotificationKind::Error);
    on_refresh(&source, "purge_irds_complete", state);

    on_event(&source, "generate_playlists_started", state, NotificationKind::Info);
    on_event(&source, "generate_playlists_error", state, NotificationKind::Error);
    on_refresh(&source, "generate_playlists_complete", state);

    on_finished(&source, "purge", move || state.purging_system_id.set(-1));
    on_finished(&source, "sort_roms", move || state.sorting_system_id.set(-1));
    on_finished(&source, "check_roms", move || state.checking_system_id.set(-1));
    on_finished(&source, "purge_irds", move || state.purging_irds_system_id.set(-1));
    on_finished(&source, "generate_playlists", move || state.generating_playlists.set(false));
    on_finished(&source, "purge_roms", move || state.purging_roms.set(false));

    // Keep the EventSource alive for the app lifetime.
    std::mem::forget(source);
}
