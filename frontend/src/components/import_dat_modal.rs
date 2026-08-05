//! Import DAT dialog with drag/drop upload (ports `ImportDatModal.svelte`).

use gloo_net::http::Request;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use web_sys::FormData;

use crate::api::report_error;
use crate::sse::DATS_ENDPOINT;
use crate::state::AppState;
use crate::ui::Modal;

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
            <div class="space-y-4 text-start">
                <div
                    class="flex cursor-pointer flex-col items-center justify-center gap-2 rounded-lg border-2 border-dashed border-gray-300 p-6 text-center hover:border-gray-400 dark:border-gray-600 dark:hover:border-gray-500"
                    on:click=move |_| open_picker()
                    on:drop=on_drop
                    on:dragover=move |ev| ev.prevent_default()
                >
                    <wa-icon name="upload" style="font-size: 2rem; color: var(--wa-color-text-quiet);"></wa-icon>
                    <Show
                        when=move || selected.get().is_some()
                        fallback=|| {
                            view! {
                                <p class="text-sm font-medium">Click or drop a file here</p>
                                <p class="text-xs text-gray-500 dark:text-gray-400">
                                    "Supported formats: .dat, .zip"
                                </p>
                            }
                        }
                    >
                        {move || {
                            let file = selected.get().unwrap();
                            view! {
                                <p class="text-sm font-medium">{file.name()}</p>
                                <p class="text-xs text-gray-500 dark:text-gray-400">
                                    {format!("{:.1} KB", file.size() / 1024.0)}
                                </p>
                            }
                        }}
                    </Show>
                </div>
                <input
                    node_ref=input_ref
                    type="file"
                    accept=".dat,.zip"
                    class="hidden"
                    on:change=on_change
                />

                <div class="flex flex-col gap-1">
                    <label class="inline-flex cursor-pointer items-center gap-2">
                        <input
                            type="checkbox"
                            class="h-4 w-4"
                            prop:checked=move || update_only.get()
                            on:change=move |ev| update_only.set(event_target_checked(&ev))
                        />
                        <span>Update only</span>
                    </label>
                    <p class="ml-6 text-sm text-gray-500 dark:text-gray-400">
                        "Only import DAT files for systems already in the database."
                    </p>
                </div>

                <div class="flex gap-2 pt-2">
                    <button
                        class="rounded-lg bg-primary-600 px-4 py-2 text-white hover:bg-primary-700 disabled:cursor-not-allowed disabled:opacity-50"
                        disabled=move || selected.get().is_none() || importing.get()
                        on:click=move |_| do_import()
                    >
                        {move || if importing.get() { "Importing…" } else { "Import" }}
                    </button>
                    <button
                        class="rounded-lg border border-gray-300 bg-white px-4 py-2 text-gray-700 hover:bg-gray-100 dark:border-gray-600 dark:bg-gray-800 dark:text-gray-300 dark:hover:bg-gray-700"
                        on:click=move |_| state.import_dat_modal_open.set(false)
                    >
                        Cancel
                    </button>
                </div>
            </div>
        </Modal>
    }
}

/// Read the `checked` state of a checkbox event target.
fn event_target_checked(ev: &web_sys::Event) -> bool {
    use wasm_bindgen::JsCast;
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|el| el.checked())
        .unwrap_or(false)
}
