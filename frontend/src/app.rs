//! Application root: provides global state, wires the reactive data-loading
//! effects that replace the Svelte `onMount` store subscriptions, kicks off the
//! initial load + SSE connection, and lays out the navbar, page and modals.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{
    get_games_by_system_id, get_roms_by_game_and_system, get_sizes_by_system_id, get_systems,
};
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

    // Initial load + real-time updates.
    connect_sse(state);
    spawn_local(async move { get_systems(state).await });

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
/// Everything that merely recomputed a view of the data — paginating, filtering,
/// counting pages — is now a memo on `AppState` that recomputes itself when its
/// sources change. What is left here is genuinely imperative: fetching from the
/// backend, and resetting the selection and paging when the user picks
/// something new.
fn setup_effects(state: AppState) {
    // Selecting a system loads its games and sizes.
    Effect::new(move |_| {
        let system_id = state.system_id.get();
        state.game_id.set(-1);
        state.games_page.set(1);
        // Drop the previous system's games so the table empties while the new
        // ones load; everything derived from them follows automatically.
        state.unfiltered_games.set(Vec::new());
        // This effect also runs once on mount, before anything is selected;
        // querying the sentinel id would just round-trip for an empty result.
        if system_id < 0 {
            return;
        }
        spawn_local(async move {
            get_games_by_system_id(state, system_id).await;
            get_sizes_by_system_id(state, system_id).await;
        });
    });

    // Selecting a game loads its roms (and with them, its romfiles).
    Effect::new(move |_| {
        let game_id = state.game_id.get();
        state.roms_page.set(1);
        state.romfiles_page.set(1);
        let system_id = state.system_id.get_untracked();
        // Clearing the roms also clears the romfiles derived from them.
        state.unfiltered_roms.set(Vec::new());
        // game_id returns to the sentinel whenever the selected system changes.
        if game_id < 0 || system_id < 0 {
            return;
        }
        spawn_local(async move {
            get_roms_by_game_and_system(state, game_id, system_id).await;
        });
    });

    // Changing a filter can shrink the list under the current page, so go back
    // to the first one. The games memo re-filters on its own.
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
