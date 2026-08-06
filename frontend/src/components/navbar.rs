//! Top navigation bar: import button, 1G1R + completion filters, name filter,
//! notifications, settings/about, dark-mode toggle. Ports the navbar portion
//! of `+layout.svelte`.

use leptos::prelude::*;

use crate::components::notifications::NotificationsButton;
use crate::state::AppState;
use crate::ui::control_value;

/// A filter toggle: filled while its rows are showing, quiet while they are
/// hidden, so the lit state means "these are on screen".
fn filter_appearance(active: bool) -> &'static str {
    if active { "filled" } else { "outlined" }
}

fn is_dark() -> bool {
    document_element()
        .map(|element| element.class_list().contains("wa-dark"))
        .unwrap_or(false)
}

fn document_element() -> Option<web_sys::Element> {
    web_sys::window()?.document()?.document_element()
}

fn set_dark(dark: bool) {
    if let Some(element) = document_element() {
        // One class for the whole app: the --wa-* tokens everything is drawn
        // with carry both light and dark values, and the components read it
        // from inside their own shadow roots.
        let classes = element.class_list();
        let _ = if dark {
            classes.add_1("wa-dark")
        } else {
            classes.remove_1("wa-dark")
        };
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
        <nav class="navbar">
            <a href="/" style="display: flex;">
                <img src="/icon.svg" alt="oxyROMon" style="height: 2rem;" />
            </a>

            <wa-button
                appearance="plain"
                title="Import DAT"
                on:click=move |_| state.import_dat_modal_open.set(true)
            >
                <wa-icon name="upload" label="Import DAT"></wa-icon>
            </wa-button>

            <div style="flex: 1;"></div>

            <wa-button
                variant="brand"
                appearance=move || filter_appearance(one_region.get())
                on:click=move |_| one_region.update(|b| *b = !*b)
            >
                {move || if one_region.get() { "1G1R only" } else { "All regions" }}
            </wa-button>

            <wa-button-group label="Completion filters">
                <wa-button
                    variant="success"
                    appearance=move || filter_appearance(complete.get())
                    on:click=move |_| complete.update(|b| *b = !*b)
                >
                    Complete
                </wa-button>
                <wa-button
                    variant="warning"
                    appearance=move || filter_appearance(incomplete.get())
                    on:click=move |_| incomplete.update(|b| *b = !*b)
                >
                    Incomplete
                </wa-button>
                <wa-button
                    variant="danger"
                    appearance=move || filter_appearance(wanted.get())
                    on:click=move |_| wanted.update(|b| *b = !*b)
                >
                    Wanted
                </wa-button>
                <wa-button
                    variant="neutral"
                    appearance=move || filter_appearance(ignored.get())
                    on:click=move |_| ignored.update(|b| *b = !*b)
                >
                    Ignored
                </wa-button>
            </wa-button-group>

            <wa-input
                type="search"
                placeholder="Game Name"
                size="small"
                style="width: 14rem;"
                prop:value=move || state.name_filter.get()
                on:input=move |ev| state.name_filter.set(control_value(&ev))
            ></wa-input>

            <NotificationsButton />

            <wa-button
                appearance="plain"
                title="Settings"
                on:click=move |_| state.settings_modal_open.update(|b| *b = !*b)
            >
                <wa-icon name="sliders" label="Settings"></wa-icon>
            </wa-button>

            <wa-button
                appearance="plain"
                title="About"
                on:click=move |_| state.about_modal_open.update(|b| *b = !*b)
            >
                <wa-icon name="circle-info" label="About"></wa-icon>
            </wa-button>

            <wa-button
                appearance="plain"
                title="Toggle dark mode"
                on:click=move |_| {
                    let next = !dark.get();
                    set_dark(next);
                    dark.set(next);
                }
            >
                <wa-icon
                    name=move || if dark.get() { "sun" } else { "moon" }
                    label="Toggle dark mode"
                ></wa-icon>
            </wa-button>
        </nav>
    }
}
