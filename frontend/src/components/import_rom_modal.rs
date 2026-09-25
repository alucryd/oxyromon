//! Import ROM dialog: upload a file, or hand the server a URL to fetch.

use gloo_net::http::Request;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use web_sys::FormData;

use crate::api::report_error;
use crate::state::AppState;
use crate::ui::{Dropzone, Modal, control_value};

/// Where the server accepts a ROM to import, by upload or by URL.
pub const ROMS_ENDPOINT: &str = "/roms";

#[component]
pub fn ImportRomModal() -> impl IntoView {
    let state = expect_context::<AppState>();
    let open = state.import_rom_modal_open;

    let importing = RwSignal::new(false);
    let selected = RwSignal::new(Option::<web_sys::File>::None);
    let url = RwSignal::new(String::new());
    let unattended = RwSignal::new("first".to_string());
    let input_ref = NodeRef::<html::Input>::new();

    let clear = move || {
        selected.set(None);
        url.set(String::new());
        if let Some(input) = input_ref.get_untracked() {
            input.set_value("");
        }
    };

    // A file and a URL are alternatives, so picking one drops the other rather
    // than leaving the dialog in a state where it is unclear which will be used.
    Effect::new(move || {
        if selected.get().is_some() {
            url.set(String::new());
        }
    });

    let ready = move || selected.get().is_some() || !url.get().trim().is_empty();

    let do_import = move || {
        let file = selected.get_untracked();
        let link = url.get_untracked().trim().to_string();
        if file.is_none() && link.is_empty() {
            return;
        }
        importing.set(true);
        spawn_local(async move {
            let form = FormData::new().unwrap();
            match &file {
                Some(file) => {
                    let _ = form.append_with_blob_and_filename("file", file, &file.name());
                }
                None => {
                    let _ = form.append_with_str("url", &link);
                }
            }
            let _ = form.append_with_str("unattended", &unattended.get_untracked());
            // The import reports progress over SSE; this only covers failures to
            // hand the job over in the first place.
            let outcome = match Request::post(ROMS_ENDPOINT).body(form) {
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
                    clear();
                    open.set(false);
                }
                // Leave the dialog as it is so the attempt can simply be retried.
                Err(e) => report_error(state.notifier, "Importing the ROM file", &e),
            }
        });
    };

    view! {
        <Modal open=open title=Signal::derive(|| "Import ROMs".to_string()) size="sm">
            <div class="wa-stack wa-gap-m">
                <Dropzone
                    icon="upload"
                    hint="Archives are imported as they are"
                    selected=selected
                    input_ref=input_ref
                />

                <wa-divider></wa-divider>

                <wa-input
                    type="url"
                    label="From a URL"
                    placeholder="https://example.com/game.zip"
                    hint="Fetched by the server, so the file never passes through this browser."
                    prop:value=move || url.get()
                    on:input=move |ev| {
                        let value = control_value(&ev);
                        if !value.is_empty() {
                            selected.set(None);
                        }
                        url.set(value);
                    }
                ></wa-input>

                <wa-select
                    label="If a file matches several games"
                    prop:value=move || unattended.get()
                    on:change=move |ev| {
                        let chosen = control_value(&ev);
                        unattended.set(chosen);
                    }
                >
                    <wa-option value="first">Import the first</wa-option>
                    <wa-option value="skip">Skip the file</wa-option>
                </wa-select>
            </div>

            <wa-button slot="footer" appearance="plain" on:click=move |_| open.set(false)>
                Cancel
            </wa-button>
            <wa-button
                slot="footer"
                variant="brand"
                appearance="filled"
                prop:disabled=move || !ready() || importing.get()
                prop:loading=move || importing.get()
                on:click=move |_| do_import()
            >
                Import
            </wa-button>
        </Modal>
    }
}
