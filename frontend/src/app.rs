//! Application root: provides global state, resets the selection and paging
//! when the user picks something new, opens the SSE connection, and lays out
//! the navbar, page and modals.

use leptos::prelude::*;

use crate::components::about_modal::AboutModal;
use crate::components::import_dat_modal::ImportDatModal;
use crate::components::navbar::Navbar;
use crate::components::settings_modal::SettingsModal;
use crate::page::Page;
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
        <div class="flex min-h-screen bg-slate-200 dark:bg-slate-800">
            <Navbar />
            <div class="flex w-full flex-col gap-4">
                <Page />
            </div>

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
