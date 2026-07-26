//! Main page: systems / games / roms / romfiles tables, statistics, the
//! delete-confirmation modal, the per-system settings modal and the toast.
//! Ports `+page.svelte`.

use std::collections::HashSet;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::purge_system;
use crate::components::settings_modal::SettingsModal;
use crate::icons::{
    ADJUSTMENTS, CHECK_CIRCLE, CLOSE_CIRCLE, DOTS_VERTICAL, DOWNLOAD, EXCLAMATION_CIRCLE, Icon,
    Spinner, TRASH,
};
use crate::model::{Game, NotificationKind, Rom, System};
use crate::state::{AppState, format_bytes};
use crate::ui::Pagination;

fn system_color(system: &System) -> &'static str {
    match system.completion {
        2 => "text-emerald-600 dark:text-emerald-400",
        1 => "text-amber-500 dark:text-amber-400",
        _ => "text-red-600 dark:text-red-400",
    }
}

fn game_color(game: &Game) -> &'static str {
    if game.sorting == 2 {
        return "text-slate-400 dark:text-slate-500";
    }
    match game.completion {
        2 => "text-emerald-600 dark:text-emerald-400",
        1 => "text-amber-500 dark:text-amber-400",
        _ => "text-red-600 dark:text-red-400",
    }
}

fn rom_color(rom: &Rom) -> &'static str {
    if rom.ignored {
        "text-slate-400 dark:text-slate-500"
    } else if rom.romfile.is_some() {
        "text-emerald-600 dark:text-emerald-400"
    } else {
        "text-red-600 dark:text-red-400"
    }
}

const CARD: &str = "flex max-w-none flex-1 flex-col overflow-hidden rounded-lg border border-gray-200 bg-white shadow-sm dark:border-gray-700 dark:bg-gray-800";
const TABLE: &str = "mb-4 w-full table-fixed text-left text-base";

#[component]
pub fn Page() -> impl IntoView {
    // Per-system settings modal state (distinct from the global one).
    let sys_settings_open = RwSignal::new(false);
    let sys_settings_id = RwSignal::new(Option::<i64>::None);
    let sys_settings_title = RwSignal::new("Settings".to_string());

    // Delete confirmation modal state.
    let delete_open = RwSignal::new(false);
    let delete_target = RwSignal::new(Option::<System>::None);

    view! {
        <div class="flex min-h-screen w-full flex-col px-4">
            <div class="mt-20 mb-4 grid flex-1 grid-cols-1 gap-4 md:grid-cols-10">
                <div class="flex flex-col md:col-span-2">
                    <SystemsCard
                        sys_settings_open=sys_settings_open
                        sys_settings_id=sys_settings_id
                        sys_settings_title=sys_settings_title
                        delete_open=delete_open
                        delete_target=delete_target
                    />
                </div>
                <div class="flex flex-col md:col-span-3">
                    <GamesCard />
                </div>
                <div class="flex flex-col gap-4 md:col-span-5">
                    <RomsCard />
                    <RomfilesCard />
                </div>
            </div>

            <StatsCard />

            <SettingsModal
                open=sys_settings_open
                system_id=sys_settings_id
                title=sys_settings_title
            />

            <DeleteModal open=delete_open target=delete_target />

            <Toast />
        </div>
    }
}

#[component]
fn SystemsCard(
    sys_settings_open: RwSignal<bool>,
    sys_settings_id: RwSignal<Option<i64>>,
    sys_settings_title: RwSignal<String>,
    delete_open: RwSignal<bool>,
    delete_target: RwSignal<Option<System>>,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    // Which system's action dropdown is open (-1 = none).
    let open_dropdown = RwSignal::new(-1_i64);

    view! {
        <div class=CARD>
            <table class=TABLE>
                <thead class="text-left text-base">
                    <tr>
                        <th class="w-full px-4 py-2">Systems</th>
                        <th class="w-8 px-2 py-2">
                            <Show when=move || state.loading_systems.get()>
                                <Spinner />
                            </Show>
                        </th>
                    </tr>
                </thead>
                <tbody>
                    <For each=move || state.systems.get() key=|s| s.id let:system>
                        {
                            let sid = system.id;
                            let name = system.name.clone();
                            let color = system_color(&system);
                            let description = if system.description != system.name {
                                system.description.clone()
                            } else {
                                String::new()
                            };
                            let selected = move || state.system_id.get() == sid;
                            let system_for_delete = system.clone();
                            let name_for_settings = name.clone();
                            view! {
                                <tr>
                                    <td class=move || {
                                        format!(
                                            "p-0 {}",
                                            if selected() { "bg-slate-200 dark:bg-slate-700" } else { "" },
                                        )
                                    }>
                                        <button
                                            class=format!(
                                                "block w-full truncate px-4 py-2 text-left text-base {color}",
                                            )
                                            title=description
                                            on:click=move |_| state.system_id.set(sid)
                                        >
                                            {name.clone()}
                                        </button>
                                    </td>
                                    <td class=move || {
                                        format!(
                                            "relative px-2 py-2 text-right {}",
                                            if selected() { "bg-slate-200 dark:bg-slate-700" } else { "" },
                                        )
                                    }>
                                        <Show
                                            when=move || state.purging_system_id.get() == sid
                                            fallback=move || {
                                                let name_for_settings = name_for_settings.clone();
                                                let system_for_delete = system_for_delete.clone();
                                                view! {
                                                    <button
                                                        class="rounded p-1 hover:bg-slate-300 dark:hover:bg-slate-600"
                                                        on:click=move |_| {
                                                            open_dropdown
                                                                .update(|d| *d = if *d == sid { -1 } else { sid });
                                                        }
                                                    >
                                                        <Icon path=DOTS_VERTICAL class="h-4 w-4" />
                                                    </button>
                                                    <Show when=move || open_dropdown.get() == sid>
                                                        {
                                                            let name_for_settings = name_for_settings.clone();
                                                            let system_for_delete = system_for_delete.clone();
                                                            view! {
                                                                <div class="absolute right-2 z-30 mt-1 w-40 overflow-hidden rounded-lg border border-gray-200 bg-white text-left shadow-lg dark:border-gray-600 dark:bg-gray-700">
                                                                    <button
                                                                        class="flex w-full items-center px-4 py-2 text-sm hover:bg-gray-100 dark:text-gray-100 dark:hover:bg-gray-600"
                                                                        on:click=move |_| {
                                                                            sys_settings_id.set(Some(sid));
                                                                            sys_settings_title
                                                                                .set(format!("{name_for_settings} Settings"));
                                                                            sys_settings_open.set(true);
                                                                            open_dropdown.set(-1);
                                                                        }
                                                                    >
                                                                        <Icon path=ADJUSTMENTS class="mr-2 inline h-4 w-4" />
                                                                        Settings
                                                                    </button>
                                                                    <button
                                                                        class="flex w-full items-center px-4 py-2 text-sm text-red-600 hover:bg-gray-100 dark:text-red-400 dark:hover:bg-gray-600"
                                                                        on:click=move |_| {
                                                                            delete_target.set(Some(system_for_delete.clone()));
                                                                            delete_open.set(true);
                                                                            open_dropdown.set(-1);
                                                                        }
                                                                    >
                                                                        <Icon path=TRASH class="mr-2 inline h-4 w-4" />
                                                                        Delete
                                                                    </button>
                                                                </div>
                                                            }
                                                        }
                                                    </Show>
                                                }
                                            }
                                        >
                                            <Spinner />
                                        </Show>
                                    </td>
                                </tr>
                            }
                        }
                    </For>
                </tbody>
            </table>
            <Pagination page=state.systems_page total_pages=state.systems_total_pages />
        </div>
    }
}

#[component]
fn GamesCard() -> impl IntoView {
    let state = expect_context::<AppState>();
    view! {
        <div class=CARD>
            <table class=TABLE>
                <thead class="text-left text-base">
                    <tr>
                        <th class="w-full px-4 py-2">Games</th>
                        <th class="w-8 px-2 py-2">
                            <Show when=move || state.loading_games.get()>
                                <Spinner />
                            </Show>
                        </th>
                    </tr>
                </thead>
                <tbody>
                    <For each=move || state.games.get() key=|g| g.id let:game>
                        {
                            let gid = game.id;
                            let name = game.name.clone();
                            let color = game_color(&game);
                            let bold = if game.sorting == 1 { "font-bold" } else { "" };
                            let description = if game.description != game.name {
                                game.description.clone()
                            } else {
                                String::new()
                            };
                            let selected = move || state.game_id.get() == gid;
                            view! {
                                <tr>
                                    <td
                                        colspan="2"
                                        class=move || {
                                            format!(
                                                "p-0 {bold} {}",
                                                if selected() { "bg-slate-200 dark:bg-slate-700" } else { "" },
                                            )
                                        }
                                    >
                                        <button
                                            class=format!(
                                                "block w-full truncate px-4 py-2 text-left text-base {color}",
                                            )
                                            title=description
                                            on:click=move |_| state.game_id.set(gid)
                                        >
                                            {name.clone()}
                                        </button>
                                    </td>
                                </tr>
                            }
                        }
                    </For>
                </tbody>
            </table>
            <Pagination page=state.games_page total_pages=state.games_total_pages />
        </div>
    }
}

#[component]
fn RomsCard() -> impl IntoView {
    let state = expect_context::<AppState>();
    view! {
        <div class=CARD>
            <table class=TABLE>
                <thead class="text-left text-base">
                    <tr>
                        <th class="w-full px-4 py-2">Roms</th>
                        <th class="w-8 px-2 py-2">
                            <Show when=move || state.loading_roms.get()>
                                <Spinner />
                            </Show>
                        </th>
                    </tr>
                </thead>
                <tbody>
                    <For each=move || state.roms.get() key=|r| r.id let:rom>
                        {
                            let name = rom.name.clone();
                            let color = rom_color(&rom);
                            view! {
                                <tr>
                                    <td
                                        colspan="2"
                                        class=format!(
                                            "truncate px-4 py-2 text-left text-base {color}",
                                        )
                                    >
                                        {name}
                                    </td>
                                </tr>
                            }
                        }
                    </For>
                </tbody>
            </table>
            <Pagination page=state.roms_page total_pages=state.roms_total_pages />
        </div>
    }
}

#[component]
fn RomfilesCard() -> impl IntoView {
    let state = expect_context::<AppState>();
    view! {
        <div class=CARD>
            <table class=TABLE>
                <thead class="text-left text-base">
                    <tr>
                        <th class="w-full px-4 py-2">Romfiles</th>
                        <th class="w-8 px-2 py-2">
                            <Show when=move || state.loading_roms.get()>
                                <Spinner />
                            </Show>
                        </th>
                    </tr>
                </thead>
                <tbody>
                    <For
                        each=move || state.romfiles.get()
                        key=|romfile| romfile.path.clone()
                        let:romfile
                    >
                        {
                            let id = romfile.id;
                            // Drop the leading system directory segment for display.
                            let display: String = romfile
                                .path
                                .split('/')
                                .skip(1)
                                .collect::<Vec<_>>()
                                .join("/");
                            view! {
                                <tr>
                                    <td class="truncate px-4 py-2 text-left text-base">{display}</td>
                                    <td class="px-2 py-2 text-right">
                                        <a
                                            href=format!("/romfiles/{id}")
                                            download
                                            class="inline-flex rounded bg-primary-600 p-1 text-white hover:bg-primary-700"
                                        >
                                            <Icon path=DOWNLOAD class="h-4 w-4" />
                                        </a>
                                    </td>
                                </tr>
                            }
                        }
                    </For>
                </tbody>
            </table>
            <Pagination page=state.romfiles_page total_pages=state.romfiles_total_pages />
        </div>
    }
}

#[component]
fn StatsCard() -> impl IntoView {
    let state = expect_context::<AppState>();

    let unique_romfiles = move || {
        let paths: HashSet<String> = state
            .unfiltered_roms
            .get()
            .into_iter()
            .filter_map(|rom| rom.romfile.map(|romfile| romfile.path))
            .collect();
        paths.len()
    };

    let tile = "rounded bg-gray-100 p-2 text-center dark:bg-gray-700";
    let value = "text-lg font-bold text-gray-900 dark:text-white";
    let label = "text-xs text-gray-500 dark:text-gray-400";

    view! {
        <div class="mb-4">
            <div class="max-w-none overflow-hidden rounded-lg border border-gray-200 bg-white shadow-sm dark:border-gray-700 dark:bg-gray-800">
                <div class="px-4 py-2 text-left text-base">Statistics</div>
                <div class="grid grid-cols-4 gap-2 p-4">
                    <div class=tile>
                        <Show
                            when=move || !state.loading_systems.get()
                            fallback=|| view! { <Spinner /> }
                        >
                            <p class=value>{move || state.unfiltered_systems.get().len()}</p>
                        </Show>
                        <p class=label>Systems</p>
                    </div>
                    <div class=tile>
                        <Show
                            when=move || !state.loading_games.get()
                            fallback=|| view! { <Spinner /> }
                        >
                            <p class=value>{move || state.unfiltered_games.get().len()}</p>
                        </Show>
                        <p class=label>Games</p>
                    </div>
                    <div class=tile>
                        <Show when=move || !state.loading_roms.get() fallback=|| view! { <Spinner /> }>
                            <p class=value>{move || state.unfiltered_roms.get().len()}</p>
                        </Show>
                        <p class=label>ROMs</p>
                    </div>
                    <div class=tile>
                        <Show when=move || !state.loading_roms.get() fallback=|| view! { <Spinner /> }>
                            <p class=value>{unique_romfiles}</p>
                        </Show>
                        <p class=label>ROM Files</p>
                    </div>
                    <div class=tile>
                        <Show
                            when=move || !state.loading_sizes.get()
                            fallback=|| view! { <Spinner /> }
                        >
                            <p class=value>
                                {move || format_bytes(state.total_original_size.get())}
                            </p>
                        </Show>
                        <p class=label>Total Size (Original)</p>
                    </div>
                    <div class=tile>
                        <Show
                            when=move || !state.loading_sizes.get()
                            fallback=|| view! { <Spinner /> }
                        >
                            <p class=value>
                                {move || format_bytes(state.one_region_original_size.get())}
                            </p>
                        </Show>
                        <p class=label>1G1R Size (Original)</p>
                    </div>
                    <div class=tile>
                        <Show
                            when=move || !state.loading_sizes.get()
                            fallback=|| view! { <Spinner /> }
                        >
                            <p class=value>{move || format_bytes(state.total_actual_size.get())}</p>
                        </Show>
                        <p class=label>Total Size (Actual)</p>
                    </div>
                    <div class=tile>
                        <Show
                            when=move || !state.loading_sizes.get()
                            fallback=|| view! { <Spinner /> }
                        >
                            <p class=value>
                                {move || format_bytes(state.one_region_actual_size.get())}
                            </p>
                        </Show>
                        <p class=label>1G1R Size (Actual)</p>
                    </div>
                </div>
            </div>
        </div>
    }
}

#[component]
fn DeleteModal(open: RwSignal<bool>, target: RwSignal<Option<System>>) -> impl IntoView {
    let state = expect_context::<AppState>();

    let name = move || target.get().map(|s| s.name).unwrap_or_default();

    let confirm = move |_| {
        if let Some(system) = target.get_untracked() {
            spawn_local(async move { purge_system(state, system.id).await });
        }
        open.set(false);
        target.set(None);
    };

    view! {
        <Show when=move || open.get()>
            <div
                class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
                on:click=move |_| open.set(false)
            >
                <div
                    class="relative w-full max-w-sm rounded-lg bg-white p-6 text-center shadow-xl dark:bg-gray-800"
                    on:click=|ev| ev.stop_propagation()
                >
                    <Icon
                        path=EXCLAMATION_CIRCLE
                        class="mx-auto mb-4 h-12 w-12 text-red-500 dark:text-red-400"
                    />
                    <h3 class="mb-5 text-lg font-normal text-slate-700 dark:text-slate-300">
                        {move || format!("Are you sure you want to delete system \"{}\"?", name())}
                    </h3>
                    <p class="mb-5 text-sm text-slate-500 dark:text-slate-400">
                        "This action cannot be undone. All data associated with this system will be permanently removed."
                    </p>
                    <button
                        class="me-2 rounded-lg bg-red-600 px-4 py-2 text-white hover:bg-red-700"
                        on:click=confirm
                    >
                        "Yes, I'm sure"
                    </button>
                    <button
                        class="rounded-lg border border-gray-300 bg-white px-4 py-2 text-gray-700 hover:bg-gray-100 dark:border-gray-600 dark:bg-gray-800 dark:text-gray-300 dark:hover:bg-gray-700"
                        on:click=move |_| {
                            open.set(false);
                            target.set(None);
                        }
                    >
                        "No, cancel"
                    </button>
                </div>
            </div>
        </Show>
    }
}

#[component]
fn Toast() -> impl IntoView {
    let state = expect_context::<AppState>();
    view! {
        <Show when=move || state.toast.get().is_some()>
            {move || {
                let notification = state.toast.get().unwrap();
                let (color, icon) = match notification.kind {
                    NotificationKind::Success => {
                        ("border-green-500 text-green-500", CHECK_CIRCLE)
                    }
                    NotificationKind::Error => ("border-red-500 text-red-500", CLOSE_CIRCLE),
                    _ => ("border-blue-500 text-blue-500", EXCLAMATION_CIRCLE),
                };
                view! {
                    <div class=format!(
                        "fixed right-4 bottom-4 z-50 flex items-center gap-3 rounded-lg border-l-4 bg-white p-4 shadow-lg dark:bg-gray-800 {color}",
                    )>
                        <Icon path=icon class="h-5 w-5" />
                        <span class="text-sm text-slate-700 dark:text-slate-200">
                            {notification.message.clone()}
                        </span>
                    </div>
                }
            }}
        </Show>
    }
}
