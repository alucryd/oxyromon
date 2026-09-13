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
        push_notification(state.notifier, message_field(&data), kind);
        if !skipped {
            // The set of systems changed underneath us, so pull it again.
            state.systems_resource.refetch();
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

    // Sorting rearranges the selected system's files, so besides the set of
    // systems the selected system's own fetches must run again: bouncing the
    // selection off its sentinel re-triggers them (Leptos skips a set to the
    // same value, which is why the -1 round-trip is there at all).
    let handler = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let data: Value = event
            .data()
            .as_string()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        push_notification(state.notifier, message_field(&data), NotificationKind::Success);
        state.systems_resource.refetch();
        let system_id = state.system_id.get();
        if system_id > 0 {
            state.system_id.set(-1);
            state.system_id.set(system_id);
        }
        let game_id = state.game_id.get();
        if game_id > 0 {
            state.game_id.set(-1);
            state.game_id.set(game_id);
        }
    });
    source
        .add_event_listener_with_callback(
            "sort_roms_complete",
            handler.as_ref().unchecked_ref(),
        )
        .ok();
    handler.forget();

    on_event(&source, "check_roms_started", state, NotificationKind::Info);
    on_event(&source, "check_roms_error", state, NotificationKind::Error);
    let handler = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let data: Value = event
            .data()
            .as_string()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        push_notification(state.notifier, message_field(&data), NotificationKind::Success);
        state.systems_resource.refetch();
        // Corrupt files were moved to the Trash directory, so the selected
        // system's files and completion may have changed: bounce the selection
        // off its sentinel to re-run its fetches.
        let system_id = state.system_id.get();
        if system_id > 0 {
            state.system_id.set(-1);
            state.system_id.set(system_id);
        }
        let game_id = state.game_id.get();
        if game_id > 0 {
            state.game_id.set(-1);
            state.game_id.set(game_id);
        }
    });
    source
        .add_event_listener_with_callback(
            "check_roms_complete",
            handler.as_ref().unchecked_ref(),
        )
        .ok();
    handler.forget();

    on_event(&source, "purge_roms_started", state, NotificationKind::Info);
    on_event(&source, "purge_roms_error", state, NotificationKind::Error);
    let handler = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let data: Value = event
            .data()
            .as_string()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        push_notification(state.notifier, message_field(&data), NotificationKind::Success);
        state.systems_resource.refetch();
        // Purged files change the systems' completion, so bounce the selection
        // off its sentinel to re-run its fetches.
        let system_id = state.system_id.get();
        if system_id > 0 {
            state.system_id.set(-1);
            state.system_id.set(system_id);
        }
        let game_id = state.game_id.get();
        if game_id > 0 {
            state.game_id.set(-1);
            state.game_id.set(game_id);
        }
    });
    source
        .add_event_listener_with_callback(
            "purge_roms_complete",
            handler.as_ref().unchecked_ref(),
        )
        .ok();
    handler.forget();

    on_event(&source, "convert_roms_started", state, NotificationKind::Info);
    on_event(&source, "convert_roms_error", state, NotificationKind::Error);
    let handler = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        let data: Value = event
            .data()
            .as_string()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        push_notification(state.notifier, message_field(&data), NotificationKind::Success);
        state.systems_resource.refetch();
        // Converted files change the system's files and completion, so bounce
        // the selection off its sentinel to re-run its fetches.
        let system_id = state.system_id.get();
        if system_id > 0 {
            state.system_id.set(-1);
            state.system_id.set(system_id);
        }
        let game_id = state.game_id.get();
        if game_id > 0 {
            state.game_id.set(-1);
            state.game_id.set(game_id);
        }
    });
    source
        .add_event_listener_with_callback(
            "convert_roms_complete",
            handler.as_ref().unchecked_ref(),
        )
        .ok();
    handler.forget();

    // Keep the EventSource alive for the app lifetime.
    std::mem::forget(source);
}
