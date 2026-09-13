//! Convert ROM files dialog: pick the destination format for one system.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::convert_roms;
use crate::state::AppState;
use crate::ui::{Modal, control_value};

/// The destination formats the server accepts, mirroring the CLI's.
const FORMATS: &[&str] = &["ORIGINAL", "7Z", "CHD", "CSO", "NSZ", "RVZ", "ZIP", "ZSO"];

#[component]
pub fn ConvertRomModal() -> impl IntoView {
    let state = expect_context::<AppState>();
    let open = state.convert_rom_modal_open;
    let system_id = state.convert_rom_system_id;

    let format = RwSignal::new("ORIGINAL".to_string());
    let converting = RwSignal::new(false);

    let do_convert = move || {
        let system_id = system_id.get_untracked();
        let format_value = format.get_untracked();
        converting.set(true);
        spawn_local(async move {
            let outcome = convert_roms(state, system_id, format_value).await;
            converting.set(false);
            if outcome.is_ok() {
                format.set("ORIGINAL".to_string());
                open.set(false);
            }
            // On failure the helper already reported the error, and the
            // selection is left as it is so the attempt can be retried.
        });
    };

    view! {
        <Modal open=open title=Signal::derive(|| "Convert ROM files".to_string()) size="sm">
            <div class="wa-stack wa-gap-m">
                <wa-select
                    label="Destination format"
                    hint="The ROM files of this system will be converted to this format."
                    prop:value=move || format.get()
                    on:change=move |ev| {
                        let chosen = control_value(&ev);
                        format.set(chosen);
                    }
                >
                    {FORMATS
                        .iter()
                        .map(|format| {
                            let value = *format;
                            view! {
                                <wa-option value=value.to_string()>{value.to_string()}</wa-option>
                            }
                        })
                        .collect_view()}
                </wa-select>
            </div>

            <wa-button slot="footer" appearance="plain" on:click=move |_| open.set(false)>
                Cancel
            </wa-button>
            <wa-button
                slot="footer"
                variant="brand"
                appearance="filled"
                prop:disabled=move || converting.get()
                prop:loading=move || converting.get()
                on:click=move |_| do_convert()
            >
                Convert
            </wa-button>
        </Modal>
    }
}
