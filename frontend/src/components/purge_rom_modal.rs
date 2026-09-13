//! Purge ROM files dialog: pick which categories to delete.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::purge_roms;
use crate::state::AppState;
use crate::ui::{Modal, control_checked};

#[component]
pub fn PurgeRomModal() -> impl IntoView {
    let state = expect_context::<AppState>();
    let open = state.purge_rom_modal_open;

    let missing = RwSignal::new(false);
    let orphan = RwSignal::new(false);
    let trash = RwSignal::new(false);
    let foreign = RwSignal::new(false);
    let purging = RwSignal::new(false);

    let none_selected = move || {
        !missing.get() && !orphan.get() && !trash.get() && !foreign.get()
    };

    let do_purge = move || {
        let missing_value = missing.get_untracked();
        let orphan_value = orphan.get_untracked();
        let trash_value = trash.get_untracked();
        let foreign_value = foreign.get_untracked();
        purging.set(true);
        spawn_local(async move {
            let outcome =
                purge_roms(state, missing_value, orphan_value, trash_value, foreign_value).await;
            purging.set(false);
            if outcome.is_ok() {
                missing.set(false);
                orphan.set(false);
                trash.set(false);
                foreign.set(false);
                open.set(false);
            }
            // On failure the helper already reported the error, and the
            // selections are left as they are so the attempt can be retried.
        });
    };

    view! {
        <Modal open=open title=Signal::derive(|| "Purge ROM files".to_string()) size="sm">
            <div class="wa-stack wa-gap-m">
                <div class="wa-stack wa-gap-s">
                    <div class="purge-group-heading">Remove from database</div>
                    <wa-checkbox
                        prop:checked=move || missing.get()
                        on:change=move |ev| missing.set(control_checked(&ev))
                    >
                        Delete missing ROM files
                    </wa-checkbox>
                    <wa-checkbox
                        prop:checked=move || orphan.get()
                        on:change=move |ev| orphan.set(control_checked(&ev))
                    >
                        Delete ROM files without an associated ROM
                    </wa-checkbox>
                </div>
                <div class="wa-stack wa-gap-s purge-danger">
                    <div class="purge-group-heading">Delete files from disk</div>
                    <wa-checkbox
                        prop:checked=move || trash.get()
                        on:change=move |ev| trash.set(control_checked(&ev))
                    >
                        Delete ROM files in the trash
                    </wa-checkbox>
                    <wa-checkbox
                        prop:checked=move || foreign.get()
                        on:change=move |ev| foreign.set(control_checked(&ev))
                    >
                        Delete ROM files unknown to the database
                    </wa-checkbox>
                </div>
            </div>

            <wa-button slot="footer" appearance="plain" on:click=move |_| open.set(false)>
                Cancel
            </wa-button>
            <wa-button
                slot="footer"
                variant="brand"
                appearance="filled"
                prop:disabled=move || none_selected() || purging.get()
                prop:loading=move || purging.get()
                on:click=move |_| do_purge()
            >
                Purge
            </wa-button>
        </Modal>
    }
}
