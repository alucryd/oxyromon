//! The loading spinner.
//!
//! Every other icon is a `<wa-icon>`, drawn from the Font Awesome set vendored
//! alongside Web Awesome. This one stays hand-rolled because it is animated and
//! sized from its class rather than named.

use leptos::prelude::*;

/// Animated loading spinner.
#[component]
pub fn Spinner(#[prop(optional, into)] class: String) -> impl IntoView {
    let class = if class.is_empty() {
        "h-4 w-4".to_string()
    } else {
        class
    };
    let class = format!("inline animate-spin text-slate-400 {class}");
    view! {
        <svg class=class xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path
                class="opacity-75"
                fill="currentColor"
                d="M4 12a8 8 0 0 1 8-8V0C5.373 0 0 5.373 0 12h4Z"
            ></path>
        </svg>
    }
}
