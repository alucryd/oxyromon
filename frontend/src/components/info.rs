//! Small building blocks shared by the three info modals: a label and its
//! figure on one line, and the byte figure itself.

use leptos::prelude::*;

/// A label and its value, one line: the row every info modal is built of.
///
/// The value is a view so a modal can put a byte formatter, a list of
/// badges, or a hash in monospace where a plain string would not do.
#[component]
pub fn InfoRow(label: &'static str, #[prop(optional)] children: Option<Children>) -> impl IntoView {
    view! {
        <div class="wa-flank wa-gap-m" style="justify-content: space-between;">
            <small style="color: var(--wa-color-text-quiet); white-space: nowrap;">{label}</small>
            <div style="text-align: end; min-width: 0; overflow-wrap: anywhere;">
                {children.map(|children| children())}
            </div>
        </div>
    }
}

/// A byte count, formatted by `<wa-format-bytes>` so it follows the
/// reader's locale, as the statistics tiles do.
#[component]
pub fn InfoBytes(value: i64) -> impl IntoView {
    view! { <wa-format-bytes prop:value=value as f64 display="short"></wa-format-bytes> }
}

/// A hash or a path: monospace, and free to break where it must.
#[component]
pub fn Mono(children: Children) -> impl IntoView {
    view! { <code style="font-size: var(--wa-font-size-xs);">{children()}</code> }
}

/// The completion of a system or a game, in the words the lists only carry
/// as a colour.
pub fn completion_word(completion: i64) -> &'static str {
    match completion {
        2 => "Complete",
        1 => "Incomplete",
        _ => "Wanted",
    }
}

/// The format of a ROM file: its extension, uppercased; a file without
/// one — a bare disc, a game folder — is named for what it is.
pub fn romfile_format(path: &str) -> String {
    match path.rsplit_once('.') {
        Some((_, extension)) if !extension.is_empty() && !extension.contains('/') => {
            extension.to_uppercase()
        }
        _ => "Disc".to_string(),
    }
}

/// What an info modal shows while its request is out, or once it failed:
/// a spinner, or the failure in words, so the dialog never opens blank.
#[component]
pub fn InfoPending(#[prop(into)] failed: Signal<bool>) -> impl IntoView {
    view! {
        <div class="wa-stack wa-align-items-center wa-gap-s" style="padding: var(--wa-space-xl) 0;">
            <Show
                when=move || !failed.get()
                fallback=|| {
                    view! {
                        <wa-icon name="circle-exclamation"></wa-icon>
                        <small style="color: var(--wa-color-text-quiet);">"Could not load this."</small>
                    }
                }
            >
                <wa-spinner style="font-size: var(--wa-font-size-2xl);"></wa-spinner>
            </Show>
        </div>
    }
}
