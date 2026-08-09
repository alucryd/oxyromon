//! Main page: systems / games / roms / romfiles tables, statistics, the
//! delete-confirmation modal, the per-system settings modal and the toast.
//! Ports `+page.svelte`.

use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::purge_system;
use crate::components::settings_modal::SettingsModal;
use crate::model::{Game, Rom, Romfile, Sizes, System};
use crate::state::AppState;
use crate::ui::{Modal, SizeTile, StatTile, control_number, use_media_query};
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

/// The modals the systems list opens. Grouped into one `Copy` struct so the
/// panes can be laid out two different ways without threading five props
/// through each of them.
#[derive(Clone, Copy)]
struct SystemModals {
    settings_open: RwSignal<bool>,
    settings_id: RwSignal<Option<i64>>,
    settings_title: RwSignal<String>,
    delete_open: RwSignal<bool>,
    delete_target: RwSignal<Option<System>>,
}

/// Widths of the two dividers, as a percentage of their container, remembered
/// across visits. The defaults reproduce the 2:3:5 split the panes had while
/// they were a fixed grid.
const OUTER_POSITION: (&str, f64) = ("panes-outer", 20.0);
const INNER_POSITION: (&str, f64) = ("panes-inner", 37.5);

fn stored_position((key, default): (&str, f64)) -> f64 {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item(key).ok().flatten())
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// Remember where a divider was dropped.
///
/// `wa-reposition` bubbles, so the outer split panel also hears the inner one
/// move; without the target check it would file the inner divider's position
/// under its own key.
fn remember_position(event: &web_sys::Event, key: &str) {
    if event.target() != event.current_target() {
        return;
    }
    let Some(position) = control_number(event, "position") else {
        return;
    };
    if let Some(storage) =
        web_sys::window().and_then(|window| window.local_storage().ok().flatten())
    {
        let _ = storage.set_item(key, &position.to_string());
    }
}

#[component]
pub fn Page() -> impl IntoView {
    let modals = SystemModals {
        // Per-system settings modal state (distinct from the global one).
        settings_open: RwSignal::new(false),
        settings_id: RwSignal::new(Option::<i64>::None),
        settings_title: RwSignal::new("Settings".to_string()),
        delete_open: RwSignal::new(false),
        delete_target: RwSignal::new(Option::<System>::None),
    };

    view! {
        <div class="page">
            <Panes modals=modals />

            <StatsCard />
        </div>

        // Outside `.page` on purpose. A wa-dialog host is `display: none` while
        // closed and `display: block` once open, so inside that flex column it
        // becomes a flex item the moment it opens and its gap shoves the page
        // behind it. Dialogs draw in the top layer regardless of where they sit.
        <SettingsModal
            open=modals.settings_open
            system_id=modals.settings_id
            title=modals.settings_title
        />

        <DeleteModal open=modals.delete_open target=modals.delete_target />

        <Toast />
    }
}

/// The three lists.
///
/// Wide enough and they are columns the user can resize; narrower and they
/// stack, because a draggable divider is meaningless once everything is the
/// full width of the window. That is a different tree rather than different
/// rules, which is why it is a `Show` and not a media query in the stylesheet.
#[component]
fn Panes(modals: SystemModals) -> impl IntoView {
    let wide = use_media_query("(min-width: 1100px)");

    view! {
        <Show
            when=move || wide.get()
            fallback=move || {
                view! {
                    <div class="panes">
                        <div class="pane">
                            <SystemsCard modals=modals />
                        </div>
                        <div class="pane">
                            <GamesCard />
                        </div>
                        <div class="pane">
                            <RomsCard />
                            <RomfilesCard />
                        </div>
                    </div>
                }
            }
        >
            <wa-split-panel
                class="panes-split"
                prop:position=stored_position(OUTER_POSITION)
                on:wa-reposition=move |event: web_sys::Event| {
                    remember_position(&event, OUTER_POSITION.0)
                }
            >
                <div slot="start" class="pane">
                    <SystemsCard modals=modals />
                </div>
                <div slot="end" class="pane-split-end">
                    <wa-split-panel
                        class="panes-split"
                        prop:position=stored_position(INNER_POSITION)
                        on:wa-reposition=move |event: web_sys::Event| {
                            remember_position(&event, INNER_POSITION.0)
                        }
                    >
                        <div slot="start" class="pane">
                            <GamesCard />
                        </div>
                        <div slot="end" class="pane">
                            <RomsCard />
                            <RomfilesCard />
                        </div>
                    </wa-split-panel>
                </div>
            </wa-split-panel>
        </Show>
    }
}

#[component]
fn SystemsCard(modals: SystemModals) -> impl IntoView {
    let SystemModals {
        settings_open: sys_settings_open,
        settings_id: sys_settings_id,
        settings_title: sys_settings_title,
        delete_open,
        delete_target,
    } = modals;
    let state = expect_context::<AppState>();
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
                                <div style="padding-inline-end: var(--wa-space-2xs);">
                                    <Show
                                        when=move || state.purging_system_id.get() == id
                                        fallback=move || {
                                            let name_for_settings = name_for_settings.clone();
                                            let system_for_delete = system_for_delete.clone();
                                            view! {
                                                // A dropdown rather than a hand-placed panel: it
                                                // draws in the top layer, so the pane's `overflow`
                                                // cannot clip it the way it clipped the old menu.
                                                <wa-dropdown>
                                                    <button
                                                        slot="trigger"
                                                        class="plain-button icon-button"
                                                        aria-label="System actions"
                                                    >
                                                        <wa-icon name="ellipsis-vertical"></wa-icon>
                                                    </button>
                                                    <wa-dropdown-item on:click=move |_| {
                                                        sys_settings_id.set(Some(id));
                                                        sys_settings_title
                                                            .set(format!("{name_for_settings} Settings"));
                                                        sys_settings_open.set(true);
                                                    }>
                                                        <wa-icon slot="icon" name="sliders"></wa-icon>
                                                        Settings
                                                    </wa-dropdown-item>
                                                    <wa-dropdown-item
                                                        variant="danger"
                                                        on:click=move |_| {
                                                            delete_target.set(Some(system_for_delete.clone()));
                                                            delete_open.set(true);
                                                        }
                                                    >
                                                        <wa-icon slot="icon" name="trash"></wa-icon>
                                                        Delete
                                                    </wa-dropdown-item>
                                                </wa-dropdown>
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
    let sized = move |value: fn(&Sizes) -> i64| Signal::derive(move || state.sizes.with(value));

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
                <SizeTile
                    label="Total Size (Original)"
                    bytes=sized(|sizes| sizes.total_original_size)
                    loading=state.loading_sizes
                />
                <SizeTile
                    label="1G1R Size (Original)"
                    bytes=sized(|sizes| sizes.one_region_original_size)
                    loading=state.loading_sizes
                />
                <SizeTile
                    label="Total Size (Actual)"
                    bytes=sized(|sizes| sizes.total_actual_size)
                    loading=state.loading_sizes
                />
                <SizeTile
                    label="1G1R Size (Actual)"
                    bytes=sized(|sizes| sizes.one_region_actual_size)
                    loading=state.loading_sizes
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

/// The stack transient notifications appear in.
///
/// One element for the whole app, and empty on purpose: `push_notification`
/// hands messages to it, and it owns their placement, timing and dismissal.
/// Previously this was a hand-placed `wa-callout` holding a single message, so
/// a second notification arriving replaced the first rather than stacking under
/// it.
#[component]
fn Toast() -> impl IntoView {
    // Bottom-end, where the hand-placed callout used to sit. The default
    // top-end would land the stack on the navbar, over the search field and the
    // notifications bell the toast is telling you to look at.
    view! { <wa-toast placement="bottom-end"></wa-toast> }
}
