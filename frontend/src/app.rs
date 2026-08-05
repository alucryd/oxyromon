//! Application root: provides global state, resets the selection and paging
//! when the user picks something new, opens the SSE connection, and lays out
//! the navbar, page and modals.

use leptos::prelude::*;

use crate::components::about_modal::AboutModal;
use crate::components::import_dat_modal::ImportDatModal;
use crate::components::navbar::Navbar;
use crate::components::settings_modal::SettingsModal;
use crate::page::Page;
#[allow(unused_imports)]
use crate::sse::connect_sse;
use crate::state::AppState;

#[component]
pub fn App() -> impl IntoView {
    let state = AppState::new();
    provide_context(state);

    setup_effects(state);

    // The resources start loading on their own; this is for the pushed updates.
    connect_sse(state);

    // Global settings modal (system_id = None).
    let global_settings_id = RwSignal::new(Option::<i64>::None);
    let global_settings_title = RwSignal::new("Settings".to_string());

    view! {
        // `wa-cloak` holds the app hidden until every custom element inside it
        // has been upgraded. Without it there is a flash where each `<wa-*>` is
        // still an unknown inline element, and the navbar renders as a run-on
        // line of unstyled text.
        <div class="wa-cloak app-shell">
            <Navbar />
            <Page />

            <AboutModal />
            <ImportDatModal />
            <SettingsModal
                open=state.settings_modal_open
                system_id=global_settings_id
                title=global_settings_title
            />
        </div>
    }
}

/// The one remaining effect.
///
/// Fetching is declarative (the resources in [`AppState`] re-run when the
/// selection they read changes) and so is everything drawn from it (memos).
/// Scroll position is owned by the list that scrolls. All that is left is the
/// selection the user's own choice invalidates: a game belongs to a system, so
/// picking a different system cannot leave it selected.
fn setup_effects(state: AppState) {
    Effect::new(move |_| {
        state.system_id.track();
        state.game_id.set(-1);
    });
}
