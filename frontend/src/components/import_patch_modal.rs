//! Import a patch dialog: pick the game and ROM to patch, then upload the file.

use std::ops::ControlFlow;

use gloo_net::http::Request;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use web_sys::FormData;

use crate::api::{fetch_roms, report_error, stream_games};
use crate::model::{Game, Rom};
use crate::state::AppState;
use crate::ui::{Modal, control_value};

/// Where the server accepts a patch for import.
const PATCHES_ENDPOINT: &str = "/patches";

#[component]
pub fn ImportPatchModal() -> impl IntoView {
    let state = expect_context::<AppState>();
    let open = state.import_patch_modal_open;
    let system_id = state.import_patch_system_id;

    let game_id = RwSignal::new(-1);
    let rom_id = RwSignal::new(-1);
    let games = RwSignal::new(Vec::<Game>::new());
    let roms = RwSignal::new(Vec::<Rom>::new());
    let selected = RwSignal::new(Option::<web_sys::File>::None);
    let uploading = RwSignal::new(false);
    let input_ref = NodeRef::<html::Input>::new();

    // Opening the dialog loads the system's games and resets the selection.
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        let sid = system_id.get_untracked();
        game_id.set(-1);
        rom_id.set(-1);
        games.set(Vec::new());
        roms.set(Vec::new());
        selected.set(None);
        if let Some(input) = input_ref.get_untracked() {
            input.set_value("");
        }
        if sid < 0 {
            return;
        }
        let notifier = state.notifier;
        let games_signal = games;
        spawn_local(async move {
            stream_games(
                notifier,
                sid,
                move |chunk| {
                    games_signal.update(|g| g.extend(chunk));
                    ControlFlow::Continue(())
                },
            )
            .await;
        });
    });

    // Selecting a game loads its ROMs.
    Effect::new(move |_| {
        let gid = game_id.get();
        let sid = system_id.get_untracked();
        rom_id.set(-1);
        if gid < 0 || sid < 0 {
            roms.set(Vec::new());
            return;
        }
        let notifier = state.notifier;
        spawn_local(async move {
            roms.set(fetch_roms(notifier, gid, sid).await);
        });
    });

    let on_change = move |_| {
        if let Some(file) = input_ref
            .get_untracked()
            .and_then(|input| input.files())
            .and_then(|files| files.get(0))
        {
            selected.set(Some(file));
        }
    };

    let do_import = move || {
        let Some(file) = selected.get_untracked() else {
            return;
        };
        let rid = rom_id.get_untracked();
        if rid < 0 {
            return;
        }
        uploading.set(true);
        spawn_local(async move {
            let form = FormData::new().unwrap();
            let _ = form.append_with_blob_and_filename("file", &file, &file.name());
            let _ = form.append_with_str("rom", &rid.to_string());
            // The import itself reports progress over SSE; this only covers
            // failures to hand the upload over in the first place.
            let outcome = match Request::post(PATCHES_ENDPOINT).body(form) {
                Ok(request) => match request.send().await {
                    Ok(response) if response.ok() => Ok(()),
                    Ok(response) => Err(format!("the server returned {}", response.status())),
                    Err(e) => Err(e.to_string()),
                },
                Err(e) => Err(e.to_string()),
            };
            uploading.set(false);

            match outcome {
                Ok(()) => {
                    selected.set(None);
                    if let Some(input) = input_ref.get_untracked() {
                        input.set_value("");
                    }
                    open.set(false);
                }
                // Leave the dialog open with the file still selected so the
                // upload can simply be retried.
                Err(e) => report_error(state.notifier, "Importing the patch", &e),
            }
        });
    };

    view! {
        <Modal open=open title=Signal::derive(|| "Import a patch".to_string()) size="sm">
            <div class="wa-stack wa-gap-m">
                <wa-select
                    label="Game"
                    prop:value=move || game_id.get().to_string()
                    on:change=move |ev| {
                        if let Ok(id) = control_value(&ev).parse::<i64>() {
                            game_id.set(id);
                        }
                    }
                >
                    {games
                        .get()
                        .into_iter()
                        .map(|game| {
                            let value = game.id.to_string();
                            let name = game.name.clone();
                            view! {
                                <wa-option value=value>{name}</wa-option>
                            }
                        })
                        .collect_view()}
                </wa-select>

                <wa-select
                    label="ROM"
                    prop:value=move || rom_id.get().to_string()
                    on:change=move |ev| {
                        if let Ok(id) = control_value(&ev).parse::<i64>() {
                            rom_id.set(id);
                        }
                    }
                >
                    {roms
                        .get()
                        .into_iter()
                        .map(|rom| {
                            let value = rom.id.to_string();
                            let name = rom.name.clone();
                            view! {
                                <wa-option value=value>{name}</wa-option>
                            }
                        })
                        .collect_view()}
                </wa-select>

                <button
                    class="plain-button dropzone"
                    on:click=move |_| {
                        if let Some(input) = input_ref.get_untracked() {
                            input.click();
                        }
                    }
                >
                    <wa-icon
                        name="puzzle-piece"
                        style="font-size: var(--wa-font-size-2xl); color: var(--wa-color-text-quiet);"
                    ></wa-icon>
                    <Show
                        when=move || selected.get().is_some()
                        fallback=|| {
                            view! {
                                <span>"Click here to choose a patch file"</span>
                                <small style="color: var(--wa-color-text-quiet);">
                                    "BPS, IPS or xdelta patch"
                                </small>
                            }
                        }
                    >
                        {move || {
                            let file = selected.get().unwrap();
                            view! {
                                <span style="font-weight: var(--wa-font-weight-semibold);">
                                    {file.name()}
                                </span>
                                <small style="color: var(--wa-color-text-quiet);">
                                    <wa-format-bytes value=file.size()></wa-format-bytes>
                                </small>
                            }
                        }}
                    </Show>
                </button>
                <input
                    node_ref=input_ref
                    type="file"
                    style="display: none;"
                    on:change=on_change
                />
            </div>

            <wa-button slot="footer" appearance="plain" on:click=move |_| open.set(false)>
                Cancel
            </wa-button>
            <wa-button
                slot="footer"
                variant="brand"
                appearance="filled"
                prop:disabled=move || selected.get().is_none() || rom_id.get() < 1 || uploading.get()
                prop:loading=move || uploading.get()
                on:click=move |_| do_import()
            >
                Import
            </wa-button>
        </Modal>
    }
}
