//! Import DAT dialog with drag/drop upload (ports `ImportDatModal.svelte`).

use gloo_net::http::Request;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use web_sys::FormData;

use crate::api::report_error;
use crate::sse::DATS_ENDPOINT;
use crate::state::AppState;
use crate::ui::{Dropzone, Modal, control_checked};

#[component]
pub fn ImportDatModal() -> impl IntoView {
    let state = expect_context::<AppState>();
    let update_only = RwSignal::new(false);
    let importing = RwSignal::new(false);
    let selected = RwSignal::new(Option::<web_sys::File>::None);
    let input_ref = NodeRef::<html::Input>::new();

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
                <Dropzone
                    icon="upload"
                    hint="Supported formats: .dat, .zip"
                    accept=".dat,.zip"
                    selected=selected
                    input_ref=input_ref
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
