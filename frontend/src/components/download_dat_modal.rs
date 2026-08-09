//! Download DAT dialog: pick Redump systems and let the server fetch them.

use std::collections::HashSet;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{download_dats, fetch_downloadable_systems};
use crate::state::AppState;
use crate::ui::{Modal, control_checked, control_value};

#[component]
pub fn DownloadDatModal() -> impl IntoView {
    let state = expect_context::<AppState>();
    let open = state.download_dat_modal_open;

    let update_only = RwSignal::new(false);
    let available = RwSignal::new(Vec::<String>::new());
    let selected = RwSignal::new(HashSet::<String>::new());
    let loading = RwSignal::new(false);
    let downloading = RwSignal::new(false);
    let filter = RwSignal::new(String::new());

    // Re-read the list whenever the dialog opens or the mode changes: the two
    // modes are complementary halves of the catalogue, and a download that has
    // since finished moves systems from one to the other.
    Effect::new(move |_| {
        let update = update_only.get();
        if !open.get() {
            return;
        }
        selected.set(HashSet::new());
        loading.set(true);
        spawn_local(async move {
            let systems = fetch_downloadable_systems(state.notifier, update).await;
            available.set(systems);
            loading.set(false);
        });
    });

    let visible = move || {
        let needle = filter.get().to_lowercase();
        available
            .get()
            .into_iter()
            .filter(|name| needle.is_empty() || name.to_lowercase().contains(&needle))
            .collect::<Vec<String>>()
    };

    // "Select all" acts on what is on screen, so it composes with the filter
    // rather than quietly reaching past it.
    let all_visible_selected = move || {
        let shown = visible();
        !shown.is_empty() && shown.iter().all(|name| selected.get().contains(name))
    };

    let toggle_all = move || {
        let shown = visible();
        selected.update(|set| {
            if shown.iter().all(|name| set.contains(name)) {
                for name in &shown {
                    set.remove(name);
                }
            } else {
                for name in shown {
                    set.insert(name);
                }
            }
        });
    };

    let toggle = move |name: String| {
        selected.update(|set| {
            if !set.remove(&name) {
                set.insert(name);
            }
        });
    };

    let start = move || {
        let systems: Vec<String> = selected.get_untracked().into_iter().collect();
        if systems.is_empty() {
            return;
        }
        downloading.set(true);
        spawn_local(async move {
            let accepted = download_dats(state, systems).await;
            downloading.set(false);
            // The download itself reports over SSE, so there is nothing left to
            // watch here once the server has taken the job.
            if accepted {
                open.set(false);
            }
        });
    };

    view! {
        <Modal open=open title=Signal::derive(|| "Download DATs".to_string()) size="sm">
            <div class="wa-stack wa-gap-m">
                <wa-switch
                    hint="Refresh the DAT files of Redump systems already in the database."
                    prop:checked=move || update_only.get()
                    on:change=move |ev| update_only.set(control_checked(&ev))
                >
                    Update only
                </wa-switch>

                <wa-input
                    type="search"
                    placeholder="Filter systems"
                    size="small"
                    prop:value=move || filter.get()
                    on:input=move |ev| filter.set(control_value(&ev))
                ></wa-input>

                <Show
                    when=move || !loading.get()
                    fallback=|| {
                        view! {
                            <div style="display: flex; justify-content: center; padding: var(--wa-space-l);">
                                <wa-spinner></wa-spinner>
                            </div>
                        }
                    }
                >
                    <Show
                        when=move || !visible().is_empty()
                        fallback=move || {
                            view! {
                                <p style="padding: var(--wa-space-m); text-align: center; color: var(--wa-color-text-quiet);">
                                    {move || {
                                        if !available.get().is_empty() {
                                            "No system matches that filter"
                                        } else if update_only.get() {
                                            "No Redump system to update yet"
                                        } else {
                                            "Every known Redump system is already imported"
                                        }
                                    }}
                                </p>
                            }
                        }
                    >
                        <div class="wa-split">
                            <small style="color: var(--wa-color-text-quiet);">
                                {move || format!("{} shown", visible().len())}
                            </small>
                            <wa-button
                                appearance="plain"
                                size="small"
                                on:click=move |_| toggle_all()
                            >
                                {move || {
                                    if all_visible_selected() { "Clear" } else { "Select all" }
                                }}
                            </wa-button>
                        </div>

                        <div class="download-list">
                            <For each=visible key=|name| name.clone() let:name>
                                {
                                    let value = name.clone();
                                    view! {
                                        <wa-checkbox
                                            class="download-item"
                                            title=name.clone()
                                            prop:checked=move || selected.get().contains(&value)
                                            on:change={
                                                let value = name.clone();
                                                move |_| toggle(value.clone())
                                            }
                                        >
                                            {name.clone()}
                                        </wa-checkbox>
                                    }
                                }
                            </For>
                        </div>
                    </Show>
                </Show>
            </div>

            <wa-button slot="footer" appearance="plain" on:click=move |_| open.set(false)>
                Cancel
            </wa-button>
            <wa-button
                slot="footer"
                variant="brand"
                appearance="filled"
                prop:disabled=move || selected.get().is_empty() || downloading.get()
                prop:loading=move || downloading.get()
                on:click=move |_| start()
            >
                {move || {
                    let count = selected.get().len();
                    let verb = if update_only.get() { "Update" } else { "Download" };
                    if count == 0 { verb.to_string() } else { format!("{verb} ({count})") }
                }}
            </wa-button>
        </Modal>
    }
}
