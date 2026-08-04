//! Top navigation bar: import button, 1G1R + completion filters, name filter,
//! notifications, settings/about, dark-mode toggle. Ports the navbar portion
//! of `+layout.svelte`.

use leptos::prelude::*;

use crate::components::notifications::NotificationsButton;
use crate::icons::{ADJUSTMENTS, INFO_CIRCLE, Icon, UPLOAD};
use crate::state::AppState;

/// Toggle button color classes, ported from `buttonClasses` in the layout.
fn filter_class(color: &str, active: bool) -> String {
    let base = "rounded-lg px-3 py-2 text-base font-medium";
    let variant = match (color, active) {
        ("blue", false) => "bg-sky-800 text-sky-300 hover:bg-sky-600",
        ("blue", true) => "bg-sky-600 text-sky-100 hover:bg-sky-400",
        ("green", false) => "bg-emerald-800 text-emerald-300 hover:bg-emerald-600",
        ("green", true) => "bg-emerald-600 text-emerald-100 hover:bg-emerald-400",
        ("yellow", false) => "bg-amber-800 text-amber-300 hover:bg-amber-600",
        ("yellow", true) => "bg-amber-600 text-amber-100 hover:bg-amber-400",
        ("red", false) => "bg-red-800 text-red-300 hover:bg-red-600",
        ("red", true) => "bg-red-600 text-red-100 hover:bg-red-400",
        ("gray", false) => "bg-slate-800 text-slate-300 hover:bg-slate-600",
        _ => "bg-slate-600 text-slate-100 hover:bg-slate-400",
    };
    format!("{base} {variant}")
}

fn is_dark() -> bool {
    document_element()
        .map(|e| e.class_list().contains("dark"))
        .unwrap_or(false)
}

fn document_element() -> Option<web_sys::Element> {
    web_sys::window()?.document()?.document_element()
}

fn set_dark(dark: bool) {
    if let Some(element) = document_element() {
        // Two classes for the one setting: Tailwind's variant reads `dark`,
        // while Web Awesome reads `wa-dark`. Its components style themselves
        // from inside shadow DOM, where our own variant cannot reach them.
        let classes = element.class_list();
        for class in ["dark", "wa-dark"] {
            let _ = if dark {
                classes.add_1(class)
            } else {
                classes.remove_1(class)
            };
        }
    }
    if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = storage.set_item("color-theme", if dark { "dark" } else { "light" });
    }
}

/// Apply the persisted (or default-dark) theme on startup.
pub fn init_theme() {
    let stored = web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|s| s.get_item("color-theme").ok().flatten());
    let dark = match stored.as_deref() {
        Some("light") => false,
        Some("dark") => true,
        _ => true, // default to dark, matching the previous default styling
    };
    set_dark(dark);
}

#[component]
pub fn Navbar() -> impl IntoView {
    let state = expect_context::<AppState>();
    let dark = RwSignal::new(is_dark());

    let one_region = state.one_region_filter;
    let complete = state.complete_filter;
    let incomplete = state.incomplete_filter;
    let wanted = state.wanted_filter;
    let ignored = state.ignored_filter;

    view! {
        <nav class="fixed start-0 top-0 z-20 flex w-full items-center gap-1 bg-slate-900 px-4 py-2 text-base text-white">
            <a href="/" class="flex gap-2">
                <img src="/icon.svg" alt="OXYROMON" style="height: 40px;" />
            </a>

            <div class="ml-4 flex items-center gap-1">
                <button
                    class="rounded-lg bg-slate-700 p-2.5 hover:bg-slate-600"
                    title="Import DAT"
                    on:click=move |_| state.import_dat_modal_open.set(true)
                >
                    <Icon path=UPLOAD class="h-5 w-5" />
                </button>
            </div>

            <div class="grow"></div>

            <div class="mx-2">
                <button
                    class=move || filter_class("blue", one_region.get())
                    on:click=move |_| one_region.update(|b| *b = !*b)
                >
                    {move || if one_region.get() { "Show All" } else { "Show 1G1R only" }}
                </button>
            </div>

            <div class="mx-2 flex gap-px">
                <button
                    class=move || filter_class("green", complete.get())
                    on:click=move |_| complete.update(|b| *b = !*b)
                >
                    {move || if complete.get() { "Hide" } else { "Show" }} " Complete"
                </button>
                <button
                    class=move || filter_class("yellow", incomplete.get())
                    on:click=move |_| incomplete.update(|b| *b = !*b)
                >
                    {move || if incomplete.get() { "Hide" } else { "Show" }} " Incomplete"
                </button>
                <button
                    class=move || filter_class("red", wanted.get())
                    on:click=move |_| wanted.update(|b| *b = !*b)
                >
                    {move || if wanted.get() { "Hide" } else { "Show" }} " Wanted"
                </button>
                <button
                    class=move || filter_class("gray", ignored.get())
                    on:click=move |_| ignored.update(|b| *b = !*b)
                >
                    {move || if ignored.get() { "Hide" } else { "Show" }} " Ignored"
                </button>
            </div>

            <div class="mx-2">
                <input
                    class="rounded-lg border border-gray-600 bg-gray-700 px-3 py-2 text-base text-white placeholder-gray-400"
                    type="search"
                    placeholder="Game Name"
                    prop:value=move || state.name_filter.get()
                    on:input=move |ev| state.name_filter.set(event_target_value(&ev))
                />
            </div>

            <NotificationsButton />

            <div class="mx-2 flex gap-px">
                <button
                    class="rounded-lg bg-slate-700 p-2.5 hover:bg-slate-600"
                    title="Settings"
                    on:click=move |_| state.settings_modal_open.update(|b| *b = !*b)
                >
                    <Icon path=ADJUSTMENTS class="h-5 w-5" />
                </button>
                <button
                    class="rounded-lg bg-slate-700 p-2.5 hover:bg-slate-600"
                    title="About"
                    on:click=move |_| state.about_modal_open.update(|b| *b = !*b)
                >
                    <Icon path=INFO_CIRCLE class="h-5 w-5" />
                </button>
            </div>

            <button
                class="rounded-lg p-2.5 text-slate-300 hover:bg-slate-700"
                title="Toggle dark mode"
                on:click=move |_| {
                    let next = !dark.get();
                    set_dark(next);
                    dark.set(next);
                }
            >
                {move || if dark.get() { "☀" } else { "🌙" }}
            </button>
        </nav>
    }
}
