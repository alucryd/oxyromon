//! Import ROM dialog: upload a file, or hand the server a URL to fetch.

use gloo_net::http::Request;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use web_sys::FormData;

use crate::api::report_error;
use crate::state::AppState;
use crate::ui::{Modal, control_value};

/// Where the server accepts a ROM to import, by upload or by URL.
pub const ROMS_ENDPOINT: &str = "/roms";

#[component]
pub fn ImportRomModal() -> impl IntoView {
    let state = expect_context::<AppState>();
    let open = state.import_rom_modal_open;

    let importing = RwSignal::new(false);
    let selected = RwSignal::new(Option::<web_sys::File>::None);
    let url = RwSignal::new(String::new());
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
    let choose_file = move |file: web_sys::File| {
        selected.set(Some(file));
        url.set(String::new());
    };

    let on_change = move |_| {
        if let Some(file) = input_ref
            .get()
            .and_then(|input| input.files())
            .and_then(|files| files.get(0))
        {
            choose_file(file);
        }
    };

    let on_drop = move |ev: web_sys::DragEvent| {
        ev.prevent_default();
        if let Some(file) = ev
            .data_transfer()
            .and_then(|transfer| transfer.files())
            .and_then(|files| files.get(0))
        {
            choose_file(file);
        }
    };

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
                <button
                    class="plain-button dropzone"
                    on:click=move |_| {
                        if let Some(input) = input_ref.get() {
                            input.click();
                        }
                    }
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
                                    "Archives are imported as they are"
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
