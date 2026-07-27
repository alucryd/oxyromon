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

/// The remaining effects.
///
/// Fetching is no longer among them: the resources in [`AppState`] re-run
/// themselves when the selection they read changes. What is left is the state
/// the user's choices invalidate — a game that belongs to the system they just
/// navigated away from, or a page number past the end of a list they just
/// filtered down.
fn setup_effects(state: AppState) {
    // Picking a system drops the game selection and returns to the first page.
    Effect::new(move |_| {
        state.system_id.track();
        state.game_id.set(-1);
        state.games_page.set(1);
    });

    // Picking a game returns its roms and romfiles to the first page.
    Effect::new(move |_| {
        state.game_id.track();
        state.roms_page.set(1);
        state.romfiles_page.set(1);
    });

    // Changing a filter can shrink the list under the current page.
    Effect::new(move |_| {
        state.complete_filter.track();
        state.incomplete_filter.track();
        state.wanted_filter.track();
        state.ignored_filter.track();
        state.one_region_filter.track();
        state.name_filter.track();
        state.games_page.set(1);
    });
}
