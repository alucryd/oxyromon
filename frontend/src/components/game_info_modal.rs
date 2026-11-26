//! Game info modal: what the game weighs — the DAT's figure, the disk's,
//! and for an ORE what it owns, shares and would weigh detached — and
//! the ROMs and ROM files that hold it.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{fetch_game_roms, fetch_game_sizes, report_error};
use crate::components::info::{InfoBytes, InfoRow, Mono, romfile_format};
use crate::model::{Game, GameSizes};
use crate::state::AppState;
use crate::ui::Modal;

/// The modal itself, driven by `open` and `target`: both lists and sizes
/// are asked when it opens, so the dialog answers for the game it was
/// opened on and never weighs the whole games list up front.
#[component]
pub fn GameInfoModal(open: RwSignal<bool>, target: RwSignal<Option<Game>>) -> impl IntoView {
    let state = expect_context::<AppState>();
    let sizes = RwSignal::new(Option::<GameSizes>::None);
    let roms = RwSignal::new(Vec::<crate::model::Rom>::new());
    let loading = RwSignal::new(false);

    // The files as the ROM rows name them: deduplicated by path, in
    // path order, whatever order the roms arrived in.
    let files = Memo::new(move |_| {
        let mut files: Vec<crate::model::Romfile> =
            roms.with(|roms| roms.iter().filter_map(|rom| rom.romfile.clone()).collect());
        files.sort_by(|a, b| a.path.cmp(&b.path));
        files.dedup_by(|a, b| a.path == b.path);
        files
    });

    Effect::new(move |_| {
        let shown = if open.get() { target.get() } else { None };
        if let Some(game) = shown {
            let id = game.id;
            sizes.set(None);
            roms.set(Vec::new());
            loading.set(true);
            // An answer belongs to this dialog only while it is open and
            // still about this same game: the weigh in particular can
            // outlive its game, and an overtaken arrival must not write
            // the old game's roms and sizes into the new one's dialog, nor
            // drop its spinner while the new game's own fetch is running.
            let still_asking = move || {
                open.get_untracked()
                    && target.with_untracked(|game| game.as_ref().is_some_and(|game| game.id == id))
            };
            spawn_local(async move {
                let fetched = fetch_game_roms(id).await;
                if !still_asking() {
                    return;
                }
                match fetched {
                    Ok(fetched) => roms.set(fetched),
                    Err(e) => report_error(state.notifier, "Loading game information", &e),
                }
                let fetched = fetch_game_sizes(id).await;
                if !still_asking() {
                    return;
                }
                match fetched {
                    Ok(fetched) => sizes.set(Some(fetched)),
                    Err(e) => report_error(state.notifier, "Loading game information", &e),
                }
                loading.set(false);
            });
        }
    });

    view! {
        <Modal
            open=open
            title=Signal::derive(move || {
                target
                    .with(|game| game.as_ref().map(|game| game.name.clone()).unwrap_or_default())
            })
            size="md"
        >
            <div class="wa-stack wa-gap-l">
                <Show
                    when=move || {
                        target.with(|game| {
                            game.as_ref()
                                .and_then(|game| game.description.clone())
                                .is_some()
                        })
                    }
                >
                    <small style="color: var(--wa-color-text-quiet);">
                        {move || {
                            target
                                .with(|game| {
                                    game.as_ref().and_then(|game| game.description.clone())
                                })
                                .unwrap_or_default()
                        }}
                    </small>
                </Show>

                <div class="wa-stack wa-gap-xs">
                    <Show when=move || sizes.get().is_some()>
                        {move || {
                            sizes.get().map(|sizes| {
                                view! {
                                    <InfoRow label="Original Size">
                                        <InfoBytes value=sizes.original_size/>
                                    </InfoRow>
                                    <InfoRow label="Actual Size">
                                        <InfoBytes value=sizes.actual_size/>
                                    </InfoRow>
                                    <Show when=move || sizes.ore_own.is_some()>
                                        <InfoRow label="Own (frees when deleted)">
                                            <InfoBytes value=sizes.ore_own.unwrap_or_default()/>
                                        </InfoRow>
                                        <InfoRow label="Share (of its stores)">
                                            <InfoBytes value=sizes.ore_share.unwrap_or_default()/>
                                        </InfoRow>
                                        <InfoRow label="Standalone (detached)">
                                            <InfoBytes value=sizes.ore_standalone.unwrap_or_default()/>
                                        </InfoRow>
                                    </Show>
                                }
                            })
                        }}
                    </Show>
                    <Show when=move || loading.get()>
                        <div class="wa-cluster wa-gap-s wa-align-items-center">
                            <wa-spinner></wa-spinner>
                            <small style="color: var(--wa-color-text-quiet);">
                                "Weighing the game…"
                            </small>
                        </div>
                    </Show>
                </div>

                <wa-divider></wa-divider>

                <div class="wa-stack wa-gap-2xs">
                    <small style="color: var(--wa-color-text-quiet);">ROMs</small>
                    <Show
                        when=move || !roms.get().is_empty()
                        fallback=move || {
                            view! {
                                <small style="color: var(--wa-color-text-quiet);">
                                    {move || if loading.get() { "—" } else { "None" }}
                                </small>
                            }
                        }
                    >
                        <div class="wa-stack wa-gap-3xs">
                            <For
                                each=move || roms.get()
                                key=|rom| rom.id
                                let:rom
                            >
                                <div
                                    class="wa-flank wa-gap-m"
                                    style="justify-content: space-between;"
                                >
                                    <span style="min-width: 0; overflow-wrap: anywhere;">
                                        {rom.name.clone()}
                                    </span>
                                    <small style="white-space: nowrap;">
                                        <InfoBytes value=rom.size/>
                                    </small>
                                </div>
                            </For>
                        </div>
                    </Show>
                </div>

                <div class="wa-stack wa-gap-2xs">
                    <small style="color: var(--wa-color-text-quiet);">ROM Files</small>
                    <Show
                        when=move || !files.with(Vec::is_empty)
                        fallback=move || {
                            view! {
                                <small style="color: var(--wa-color-text-quiet);">
                                    {move || if loading.get() { "—" } else { "None" }}
                                </small>
                            }
                        }
                    >
                        <div class="wa-stack wa-gap-3xs">
                            <For
                                each=move || files.get()
                                key=|file| file.id
                                let:file
                            >
                                {
                                    let file_format = romfile_format(&file.path);
                                    view! {
                                        <div
                                            class="wa-flank wa-gap-m"
                                            style="justify-content: space-between;"
                                        >
                                            <Mono>{file.path.clone()}</Mono>
                                            <div
                                                class="wa-cluster wa-gap-s"
                                                style="white-space: nowrap;"
                                            >
                                                <wa-badge appearance="outlined" pill="">
                                                    {file_format}
                                                </wa-badge>
                                                <small><InfoBytes value=file.size/></small>
                                            </div>
                                        </div>
                                    }
                                }
                            </For>
                        </div>
                    </Show>
                </div>
            </div>
        </Modal>
    }
}
