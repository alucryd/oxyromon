//! Import a PlayStation 3 IRD dialog: pick the file, the target system is the
//! one the dialog was opened for, and the game is matched automatically.

use gloo_net::http::Request;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use web_sys::FormData;

use crate::api::report_error;
use crate::state::AppState;
use crate::ui::{Dropzone, Modal};

/// Where the server accepts an IRD for import.
const IRDS_ENDPOINT: &str = "/irds";

#[component]
pub fn ImportIrdModal() -> impl IntoView {
    let state = expect_context::<AppState>();
    let open = state.import_ird_modal_open;
    let system_id = state.import_ird_system_id;

    let selected = RwSignal::new(Option::<web_sys::File>::None);
    let uploading = RwSignal::new(false);
    let input_ref = NodeRef::<html::Input>::new();

    // Opening the dialog resets the selection.
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        selected.set(None);
        if let Some(input) = input_ref.get_untracked() {
            input.set_value("");
        }
    });

    let do_import = move || {
        let Some(file) = selected.get_untracked() else {
            return;
        };
        let sid = system_id.get_untracked();
        if sid < 0 {
            return;
        }
        uploading.set(true);
        spawn_local(async move {
            let form = FormData::new().unwrap();
            let _ = form.append_with_blob_and_filename("file", &file, &file.name());
            let _ = form.append_with_str("system", &sid.to_string());
            // The import itself reports progress over SSE; this only covers
            // failures to hand the upload over in the first place.
            let outcome = match Request::post(IRDS_ENDPOINT).body(form) {
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
                Err(e) => report_error(state.notifier, "Importing the IRD", &e),
            }
        });
    };

    view! {
        <Modal open=open title=Signal::derive(|| "Import an IRD".to_string()) size="sm">
            <div class="wa-stack wa-gap-m">
                <p style="margin: 0; color: var(--wa-color-text-quiet);">
                    "A PlayStation 3 IRD describes a JB folder. The matching game is picked
                    automatically — import the system's DAT first."
                </p>
                <Dropzone
                    icon="database"
                    hint="PlayStation 3 IRD, or gzipped"
                    selected=selected
                    input_ref=input_ref
                />
            </div>

            <wa-button slot="footer" appearance="plain" on:click=move |_| open.set(false)>
                Cancel
            </wa-button>
            <wa-button
                slot="footer"
                variant="brand"
                appearance="filled"
                prop:disabled=move || selected.get().is_none() || uploading.get()
                prop:loading=move || uploading.get()
                on:click=move |_| do_import()
            >
                Import
            </wa-button>
        </Modal>
    }
}
