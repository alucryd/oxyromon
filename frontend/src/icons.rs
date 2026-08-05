//! The few inline SVG icons still drawn by hand.
//!
//! Everything else now uses `<wa-icon>`, whose icons are vendored from Font
//! Awesome. What is left are the ones inside the toast and the spinner, which
//! have not been converted yet.

use leptos::prelude::*;

// Path data (viewBox 0 0 24 24).
pub const EXCLAMATION_CIRCLE: &str =
    "M12 9v3.75m9-.75a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9 3.75h.008v.008H12v-.008Z";
pub const CHECK_CIRCLE: &str = "M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z";
pub const CLOSE_CIRCLE: &str =
    "m9.75 9.75 4.5 4.5m0-4.5-4.5 4.5M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z";

#[component]
pub fn Icon(
    /// SVG path data (one of the `pub const` strings in this module).
    #[prop(into)]
    path: &'static str,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    let class = if class.is_empty() {
        "h-4 w-4".to_string()
    } else {
        class
    };
    view! {
        <svg
            xmlns="http://www.w3.org/2000/svg"
            fill="none"
            viewBox="0 0 24 24"
            stroke-width="1.5"
            stroke="currentColor"
            class=class
        >
            <path stroke-linecap="round" stroke-linejoin="round" d=path></path>
        </svg>
    }
}

/// Animated loading spinner (replaces flowbite's `Spinner`).
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
