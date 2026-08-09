//! User-facing notifications: the bell dropdown history plus a transient toast.

use std::sync::atomic::{AtomicU64, Ordering};

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::model::{Notification, NotificationKind};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

/// How long a toast stays up. Info messages are the least consequential, so
/// they go first; everything else gets the 5s the accessibility guidance for
/// `<wa-toast>` asks for as a minimum.
const INFO_DURATION: f64 = 3000.0;
const DURATION: f64 = 5000.0;

/// The notification history.
///
/// Split out of `AppState` because the resource fetchers need to report
/// failures while `AppState` is still being constructed around them.
///
/// Transient toasts are deliberately not in here: `<wa-toast>` owns the stack
/// it shows, down to removing each item from the DOM once it has faded, so
/// mirroring that in a signal would only be something to keep in sync.
#[derive(Clone, Copy)]
pub struct Notifier {
    pub notifications: RwSignal<Vec<Notification>>,
}

impl Notifier {
    pub fn new() -> Self {
        Self {
            notifications: RwSignal::new(Vec::new()),
        }
    }
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
}

/// The current instant, ISO 8601, which is the form `<wa-relative-time>` asks
/// for so it reads the timezone correctly.
fn now_time() -> String {
    js_sys::Date::new_0()
        .to_iso_string()
        .as_string()
        .unwrap_or_default()
}

/// Hand a message to the page's `<wa-toast>` stack.
///
/// The stack handles its own placement, timing, stacking and dismissal, so
/// there is nothing to drive from here beyond the call.
///
/// It does have to wait for the element to exist as a component, though. Web
/// Awesome's autoloader fetches each component the first time its tag appears,
/// and the first notifications tend to be load failures raised while that fetch
/// is still in flight — at which point `<wa-toast>` is in the DOM but is still
/// an unknown element with no `create` on it.
fn show_toast(message: String, kind: NotificationKind) {
    spawn_local(async move {
        if let Some(window) = web_sys::window()
            && let Ok(defined) = window.custom_elements().when_defined("wa-toast")
        {
            let _ = JsFuture::from(defined).await;
        }

        let Some(toast) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.query_selector("wa-toast").ok().flatten())
        else {
            return;
        };

        let options = js_sys::Object::new();
        let set = |key: &str, value: JsValue| {
            let _ = js_sys::Reflect::set(&options, &JsValue::from_str(key), &value);
        };
        set("variant", JsValue::from_str(kind.variant()));
        set("icon", JsValue::from_str(kind.icon()));
        set(
            "duration",
            JsValue::from_f64(if kind == NotificationKind::Info {
                INFO_DURATION
            } else {
                DURATION
            }),
        );
        // Messages carry tool output and error text, so they are never markup.
        set("allowHtml", JsValue::FALSE);

        if let Ok(create) = js_sys::Reflect::get(&toast, &JsValue::from_str("create"))
            && let Some(create) = create.dyn_ref::<js_sys::Function>()
        {
            let _ = create.call2(&toast, &JsValue::from_str(&message), &options);
        }
    });
}

/// Prepend a notification and surface it as a transient toast.
pub fn push_notification(notifier: Notifier, message: String, kind: NotificationKind) {
    let notification = Notification {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        message,
        kind,
        time: now_time(),
    };
    show_toast(notification.message.clone(), kind);
    notifier.notifications.update(|list| {
        list.insert(0, notification);
    });
}
