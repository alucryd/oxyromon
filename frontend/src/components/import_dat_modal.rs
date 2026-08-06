//! Import DAT dialog with drag/drop upload (ports `ImportDatModal.svelte`).

use gloo_net::http::Request;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use web_sys::FormData;

use crate::api::report_error;
use crate::sse::DATS_ENDPOINT;
use crate::state::AppState;
use crate::ui::{Modal, control_checked};

#[component]
pub fn ImportDatModal() -> impl IntoView {
    let state = expect_context::<AppState>();
    let update_only = RwSignal::new(false);
    let importing = RwSignal::new(false);
    let selected = RwSignal::new(Option::<web_sys::File>::None);
    let input_ref = NodeRef::<html::Input>::new();

    let open_picker = move || {
        if let Some(input) = input_ref.get() {
            input.click();
        }
    };

    let on_change = move |_| {
        if let Some(file) = input_ref
            .get()
            .and_then(|input| input.files())
            .and_then(|files| files.get(0))
        {
            selected.set(Some(file));
        }
    };

    let on_drop = move |ev: web_sys::DragEvent| {
        ev.prevent_default();
        if let Some(file) = ev
            .data_transfer()
            .and_then(|dt| dt.files())
            .and_then(|files| files.get(0))
        {
            selected.set(Some(file));
        }
    };

    let do_import = move || {
        let Some(file) = selected.get_untracked() else {
            return;
        };
        importing.set(true);
        let update = update_only.get_untracked();
        spawn_local(async move {
            let form = FormData::new().unwrap();
            let _ = form.append_with_blob_and_filename("file", &file, &file.name());
            let _ = form.append_with_str("update", &update.to_string());
            // The import itself reports progress over SSE; this only covers
            // failures to hand the upload over in the first place.
            let outcome = match Request::post(DATS_ENDPOINT).body(form) {
                Ok(request) => match request.send().await {
                    Ok(response) if response.ok() => Ok(()),
                    Ok(response) => Err(format!("the server returned {}", response.status())),
                    Err(e) => Err(e.to_string()),
                },
                Err(e) => Err(e.to_string()),
            };
            importing.set(false);

            match outcome {
                Ok(()) => {
                    selected.set(None);
                    if let Some(input) = input_ref.get_untracked() {
                        input.set_value("");
                    }
                    state.import_dat_modal_open.set(false);
                }
                // Leave the dialog open with the file still selected so the
                // upload can simply be retried.
                Err(e) => report_error(state.notifier, "Uploading the DAT file", &e),
            }
        });
    };

    view! {
        <Modal
            open=state.import_dat_modal_open
            title=Signal::derive(|| "Import DAT".to_string())
            size="sm"
        >
            <div class="wa-stack wa-gap-m">
                <button
                    class="plain-button dropzone"
                    on:click=move |_| open_picker()
                    on:drop=on_drop
                    on:dragover=move |ev| ev.prevent_default()
                >
                    <wa-icon
                        name="upload"
                        style="font-size: var(--wa-font-size-2xl); color: var(--wa-color-text-quiet);"
                    ></wa-icon>
                    <Show
                        when=move || selected.get().is_some()
                        fallback=|| {
                            view! {
                                <span>Click or drop a file here</span>
                                <small style="color: var(--wa-color-text-quiet);">
                                    "Supported formats: .dat, .zip"
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
                                    {format!("{:.1} KB", file.size() / 1024.0)}
                                </small>
                            }
                        }}
                    </Show>
                </button>
                <input
                    node_ref=input_ref
                    type="file"
                    accept=".dat,.zip"
                    style="display: none;"
                    on:change=on_change
                />

                <wa-switch
                    hint="Only import DAT files for systems already in the database."
                    prop:checked=move || update_only.get()
                    on:change=move |ev| update_only.set(control_checked(&ev))
                >
                    Update only
                </wa-switch>
            </div>

            <wa-button
                slot="footer"
                appearance="plain"
                on:click=move |_| state.import_dat_modal_open.set(false)
            >
                Cancel
            </wa-button>
            <wa-button
                slot="footer"
                variant="brand"
                appearance="filled"
                prop:disabled=move || selected.get().is_none() || importing.get()
                prop:loading=move || importing.get()
                on:click=move |_| do_import()
            >
                Import
            </wa-button>
        </Modal>
    }
}
