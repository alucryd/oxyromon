//! Application root: provides global state, wires the reactive data-loading
//! effects that replace the Svelte `onMount` store subscriptions, kicks off the
//! initial load + SSE connection, and lays out the navbar, page and modals.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{
    get_games_by_system_id, get_roms_by_game_and_system, get_sizes_by_system_id, get_systems,
    update_games, update_romfiles, update_roms, update_systems,
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

/// Reactive equivalents of the `store.subscribe(...)` blocks in `+page.svelte`.
fn setup_effects(state: AppState) {
    // Pagination of the (static) systems list.
    Effect::new(move |_| {
        state.systems_page.track();
        update_systems(state);
    });

    // Selecting a system resets the games/roms panes and loads fresh data.
    Effect::new(move |_| {
        let system_id = state.system_id.get();
        state.game_id.set(-1);
        state.games_page.set(1);
        state.games.set(Vec::new());
        state.roms.set(Vec::new());
        state.romfiles.set(Vec::new());
        spawn_local(async move {
            get_games_by_system_id(state, system_id).await;
            get_sizes_by_system_id(state, system_id).await;
        });
    });

    Effect::new(move |_| {
        state.games_page.track();
        update_games(state);
    });

    // Selecting a game reloads its roms/romfiles.
    Effect::new(move |_| {
        let game_id = state.game_id.get();
        state.roms_page.set(1);
        state.romfiles_page.set(1);
        state.roms.set(Vec::new());
        state.romfiles.set(Vec::new());
        let system_id = state.system_id.get_untracked();
        spawn_local(async move {
            get_roms_by_game_and_system(state, game_id, system_id).await;
        });
    });

    Effect::new(move |_| {
        state.roms_page.track();
        update_roms(state);
    });

    Effect::new(move |_| {
        state.romfiles_page.track();
        update_romfiles(state);
    });

    // Any game filter change jumps back to page 1 (or re-filters if already there).
    for filter in [
        state.complete_filter,
        state.incomplete_filter,
        state.wanted_filter,
        state.ignored_filter,
        state.one_region_filter,
    ] {
        Effect::new(move |_| {
            filter.track();
            if state.games_page.get_untracked() != 1 {
                state.games_page.set(1);
            } else {
                update_games(state);
            }
        });
    }

    Effect::new(move |_| {
        state.name_filter.track();
        if state.games_page.get_untracked() != 1 {
            state.games_page.set(1);
        } else {
            update_games(state);
        }
    });
}
