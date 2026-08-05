//! Main page: systems / games / roms / romfiles tables, statistics, the
//! delete-confirmation modal, the per-system settings modal and the toast.
//! Ports `+page.svelte`.

use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::purge_system;
use crate::components::settings_modal::SettingsModal;
use crate::model::{Game, NotificationKind, Rom, Romfile, Sizes, System};
use crate::state::{AppState, format_bytes};
use crate::ui::{Modal, StatTile};
use crate::ui::{ScrollWindow, Spacer};

fn system_color(system: &System) -> &'static str {
    match system.completion {
        2 => "status-complete",
        1 => "status-incomplete",
        _ => "status-wanted",
    }
}

fn game_color(game: &Game) -> &'static str {
    if game.sorting == 2 {
        return "status-ignored";
    }
    match game.completion {
        2 => "status-complete",
        1 => "status-incomplete",
        _ => "status-wanted",
    }
}

fn rom_color(rom: &Rom) -> &'static str {
    if rom.ignored {
        "status-ignored"
    } else if rom.romfile.is_some() {
        "status-complete"
    } else {
        "status-wanted"
    }
}

/// Classes for one row: fixed height (the virtualized list depends on it),
/// zebra striping keyed off the row's absolute position so the stripes do not
/// shift as rows are recycled, and the selected state on top.
fn row_class(position: usize, selected: bool) -> String {
    let stripe = if position.is_multiple_of(2) {
        "row-even"
    } else {
        "row-odd"
    };
    let selected = if selected { "row-selected" } else { "" };
    format!("list-row {stripe} {selected}")
}

#[component]
fn CardHeader(title: &'static str, loading: RwSignal<bool>) -> impl IntoView {
    view! {
        <div class="panel-header">
            <span>{title}</span>
            <Show when=move || loading.get()>
                <wa-spinner></wa-spinner>
            </Show>
        </div>
    }
}

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
        <div class="page">
            <div class="panes">
                <div class="pane">
                    <SystemsCard
                        sys_settings_open=sys_settings_open
                        sys_settings_id=sys_settings_id
                        sys_settings_title=sys_settings_title
                        delete_open=delete_open
                        delete_target=delete_target
                    />
                </div>
                <div class="pane">
                    <GamesCard />
                </div>
                <div class="pane">
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
    let rows = move || {
        let systems: Vec<(usize, System)> = state.systems.get().into_iter().enumerate().collect();
        systems
    };

    view! {
        <div class="panel">
            <CardHeader title="Systems" loading=state.loading_systems />
            <div class="panel-body">
                <For each=rows key=|(_, system)| system.id let:entry>
                    {
                        let (position, system) = entry;
                        let id = system.id;
                        let name = system.name.clone();
                        let color = system_color(&system);
                        let description = if system.description != system.name {
                            system.description.clone()
                        } else {
                            String::new()
                        };
                        let selected = move || state.system_id.get() == id;
                        let system_for_delete = system.clone();
                        let name_for_settings = name.clone();
                        view! {
                            <div class=move || row_class(position, selected())>
                                <button
                                    class=format!("plain-button row-label {color}")
                                    title=description
                                    aria-current=move || selected().then_some("true")
                                    on:click=move |_| state.system_id.set(id)
                                >
                                    {name.clone()}
                                </button>
                                <div style="position: relative; padding-inline-end: var(--wa-space-2xs);">
                                    <Show
                                        when=move || state.purging_system_id.get() == id
                                        fallback=move || {
                                            let name_for_settings = name_for_settings.clone();
                                            let system_for_delete = system_for_delete.clone();
                                            view! {
                                                <button
                                                    class="plain-button icon-button"
                                                    aria-label="System actions"
                                                    on:click=move |_| {
                                                        open_dropdown.update(|d| *d = if *d == id { -1 } else { id });
                                                    }
                                                >
                                                    <wa-icon name="ellipsis-vertical"></wa-icon>
                                                </button>
                                                <Show when=move || open_dropdown.get() == id>
                                                    {
                                                        let name_for_settings = name_for_settings.clone();
                                                        let system_for_delete = system_for_delete.clone();
                                                        view! {
                                                            // Closes when the click lands anywhere else.
                                                            <div
                                                                class="overlay"
                                                                on:click=move |_| open_dropdown.set(-1)
                                                            ></div>
                                                            <div class="menu">
                                                                <button
                                                                    class="plain-button menu-item"
                                                                    on:click=move |_| {
                                                                        sys_settings_id.set(Some(id));
                                                                        sys_settings_title
                                                                            .set(format!("{name_for_settings} Settings"));
                                                                        sys_settings_open.set(true);
                                                                        open_dropdown.set(-1);
                                                                    }
                                                                >
                                                                    <wa-icon name="sliders"></wa-icon>
                                                                    Settings
                                                                </button>
                                                                <button
                                                                    class="plain-button menu-item danger"
                                                                    on:click=move |_| {
                                                                        delete_target.set(Some(system_for_delete.clone()));
                                                                        delete_open.set(true);
                                                                        open_dropdown.set(-1);
                                                                    }
                                                                >
                                                                    <wa-icon name="trash"></wa-icon>
                                                                    Delete
                                                                </button>
                                                            </div>
                                                        }
                                                    }
                                                </Show>
                                            }
                                        }
                                    >
                                        <wa-spinner></wa-spinner>
                                    </Show>
                                </div>
                            </div>
                        }
                    }
                </For>
            </div>
        </div>
    }
}

#[component]
fn GamesCard() -> impl IntoView {
    let state = expect_context::<AppState>();
    let window = ScrollWindow::new();
    let viewport = NodeRef::<html::Div>::new();

    let total = move || state.filtered_games.with(Vec::len);
    let range = Memo::new(move |_| window.range(total()));

    // Only the rows inside the window are cloned out of the resource; the rest
    // of the list is represented by the spacers above and below.
    let rows = Memo::new(move |_| {
        let (start, end) = range.get();
        state.filtered_games.with(|indices| {
            state.games.with(|games| {
                indices[start..end]
                    .iter()
                    .enumerate()
                    .map(|(offset, &index)| (start + offset, games[index].clone()))
                    .collect::<Vec<_>>()
            })
        })
    });

    // Measure once the element exists, and again whenever the window resizes,
    // so the visible row count is never stale.
    Effect::new(move |_| {
        if let Some(element) = viewport.get() {
            window.measure(&element);
        }
    });
    window_event_listener(leptos::ev::resize, move |_| {
        if let Some(element) = viewport.get_untracked() {
            window.measure(&element);
        }
    });

    // A different system, or different filters, means a different list: go back
    // to the top rather than leaving the view stranded mid-way down.
    Effect::new(move |_| {
        state.system_id.track();
        state.complete_filter.track();
        state.incomplete_filter.track();
        state.wanted_filter.track();
        state.ignored_filter.track();
        state.one_region_filter.track();
        state.name_filter.track();
        if let Some(element) = viewport.get_untracked() {
            element.set_scroll_top(0);
        }
        window.reset();
    });

    view! {
        <div class="panel">
            <CardHeader title="Games" loading=state.loading_games />
            <div
                node_ref=viewport
                class="panel-body"
                on:scroll=move |_| {
                    if let Some(element) = viewport.get_untracked() {
                        window.measure(&element);
                    }
                }
            >
                <Spacer rows=Signal::derive(move || range.get().0) />
                <For each=move || rows.get() key=|(_, game)| game.id let:entry>
                    {
                        let (position, game) = entry;
                        let id = game.id;
                        let name = game.name.clone();
                        let color = game_color(&game);
                        let weight = if game.sorting == 1 { "row-elected" } else { "" };
                        let description = game.description.clone().unwrap_or_default();
                        let selected = move || state.game_id.get() == id;
                        view! {
                            <div class=move || row_class(position, selected())>
                                <button
                                    class=format!("plain-button row-label {weight} {color}")
                                    title=description
                                    aria-current=move || selected().then_some("true")
                                    on:click=move |_| state.game_id.set(id)
                                >
                                    {name.clone()}
                                </button>
                            </div>
                        }
                    }
                </For>
                <Spacer rows=Signal::derive(move || total().saturating_sub(range.get().1)) />
            </div>
        </div>
    }
}

#[component]
fn RomsCard() -> impl IntoView {
    let state = expect_context::<AppState>();
    let rows = move || {
        let roms: Vec<(usize, Rom)> = state.roms.get().into_iter().enumerate().collect();
        roms
    };
    view! {
        <div class="panel">
            <CardHeader title="ROMs" loading=state.loading_roms />
            <div class="panel-body">
                <For each=rows key=|(_, rom)| rom.id let:entry>
                    {
                        let (position, rom) = entry;
                        let name = rom.name.clone();
                        let color = rom_color(&rom);
                        view! {
                            <div class=move || row_class(position, false)>
                                <span class=format!("row-label {color}")>
                                    {name}
                                </span>
                            </div>
                        }
                    }
                </For>
            </div>
        </div>
    }
}

#[component]
fn RomfilesCard() -> impl IntoView {
    let state = expect_context::<AppState>();
    let rows = move || {
        let romfiles: Vec<(usize, Romfile)> =
            state.romfiles.get().into_iter().enumerate().collect();
        romfiles
    };
    view! {
        <div class="panel">
            <CardHeader title="ROM Files" loading=state.loading_roms />
            <div class="panel-body">
                <For each=rows key=|(_, romfile)| romfile.path.clone() let:entry>
                    {
                        let (position, romfile) = entry;
                        let id = romfile.id;
                        // Drop the leading system directory segment for display.
                        let display: String = romfile
                            .path
                            .split('/')
                            .skip(1)
                            .collect::<Vec<_>>()
                            .join("/");
                        view! {
                            <div class=move || row_class(position, false)>
                                <span class="row-label">{display}</span>
                                <a
                                    href=format!("/romfiles/{id}")
                                    download
                                    aria-label="Download"
                                    class="icon-button" style="margin-inline-end: var(--wa-space-2xs);"
                                >
                                    <wa-icon name="download"></wa-icon>
                                </a>
                            </div>
                        }
                    }
                </For>
            </div>
        </div>
    }
}

#[component]
fn StatsCard() -> impl IntoView {
    let state = expect_context::<AppState>();

    // A figure, or a dash while the number behind it is still being fetched.
    let counted = move |loading: RwSignal<bool>, count: Memo<usize>| {
        Signal::derive(move || {
            if loading.get() {
                "—".to_string()
            } else {
                count.get().to_string()
            }
        })
    };
    let sized = move |value: fn(&Sizes) -> i64| {
        Signal::derive(move || {
            if state.loading_sizes.get() {
                "—".to_string()
            } else {
                state.sizes.with(|sizes| format_bytes(value(sizes)))
            }
        })
    };

    view! {
        <div class="panel" style="flex: none;">
            <div class="panel-header">Statistics</div>
            <div
                class="wa-grid wa-gap-s"
                style="--min-column-size: 10rem; padding: var(--wa-space-m);"
            >
                <StatTile
                    label="Systems"
                    value=counted(state.loading_systems, state.system_count)
                />
                <StatTile label="Games" value=counted(state.loading_games, state.game_count) />
                <StatTile label="ROMs" value=counted(state.loading_roms, state.rom_count) />
                <StatTile
                    label="ROM Files"
                    value=Signal::derive(move || {
                        if state.loading_roms.get() {
                            "—".to_string()
                        } else {
                            state.romfiles.with(Vec::len).to_string()
                        }
                    })
                />
                <StatTile
                    label="Total Size (Original)"
                    value=sized(|sizes| sizes.total_original_size)
                />
                <StatTile
                    label="1G1R Size (Original)"
                    value=sized(|sizes| sizes.one_region_original_size)
                />
                <StatTile label="Total Size (Actual)" value=sized(|sizes| sizes.total_actual_size) />
                <StatTile
                    label="1G1R Size (Actual)"
                    value=sized(|sizes| sizes.one_region_actual_size)
                />
            </div>
        </div>
    }
}

#[component]
fn DeleteModal(open: RwSignal<bool>, target: RwSignal<Option<System>>) -> impl IntoView {
    let state = expect_context::<AppState>();
    let name = move || target.get().map(|system| system.name).unwrap_or_default();

    let confirm = move |_| {
        if let Some(system) = target.get_untracked() {
            spawn_local(async move { purge_system(state, system.id).await });
        }
        open.set(false);
        target.set(None);
    };

    view! {
        <Modal open=open title=Signal::derive(|| "Delete system".to_string()) size="xs">
            <div class="wa-stack wa-gap-m">
                <wa-callout variant="danger">
                    <wa-icon slot="icon" name="triangle-exclamation"></wa-icon>
                    {move || {
                        format!(
                            "Everything oxyromon knows about \"{}\" will be removed. Files on disk are left alone.",
                            name(),
                        )
                    }}
                </wa-callout>
            </div>
            <wa-button slot="footer" appearance="plain" on:click=move |_| open.set(false)>
                Cancel
            </wa-button>
            <wa-button slot="footer" variant="danger" appearance="filled" on:click=confirm>
                Delete
            </wa-button>
        </Modal>
    }
}

#[component]
fn Toast() -> impl IntoView {
    let state = expect_context::<AppState>();
    view! {
        <Show when=move || state.notifier.toast.get().is_some()>
            {move || {
                let notification = state.notifier.toast.get().unwrap();
                let (variant, icon) = match notification.kind {
                    NotificationKind::Success => ("success", "circle-check"),
                    NotificationKind::Warning => ("warning", "triangle-exclamation"),
                    NotificationKind::Error => ("danger", "circle-exclamation"),
                    NotificationKind::Info => ("brand", "circle-info"),
                };
                view! {
                    <div style="position: fixed; inset-block-end: var(--wa-space-m); inset-inline-end: var(--wa-space-m); z-index: 60; max-width: 28rem;">
                        <wa-callout variant=variant appearance="filled-outlined">
                            <wa-icon slot="icon" name=icon></wa-icon>
                            {notification.message.clone()}
                        </wa-callout>
                    </div>
                }
            }}
        </Show>
    }
}
