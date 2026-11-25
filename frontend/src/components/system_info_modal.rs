//! System info modal: the DAT it was imported from, what it holds and
//! weighs and how complete it is, and — when its games live in an ORE
//! store — what that store weighs, saves and costs, this system's part
//! of it, and the systems it shares it with.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{fetch_system_details, report_error};
use crate::components::info::{InfoBytes, InfoRow, Mono, completion_word};
use crate::model::{SystemDetails, SystemFacts};
use crate::state::AppState;
use crate::ui::Modal;

/// The modal itself, driven by `open` and `system_id`: everything it shows
/// is asked when it opens and thrown away when the system changes, so a
/// reopened dialog answers for the system it was opened on, not the one
/// opened last.
#[component]
pub fn SystemInfoModal(
    open: RwSignal<bool>,
    system_id: RwSignal<i64>,
    title: RwSignal<String>,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    let details = RwSignal::new(Option::<SystemDetails>::None);

    // Ask each time the modal opens: the DAT, the sizes and the store move
    // between visits — a check, an import, a conversion — and one request
    // is cheap next to being wrong.
    Effect::new(move |_| {
        let shown = if open.get() {
            Some(system_id.get())
        } else {
            None
        };
        if let Some(id) = shown {
            details.set(None);
            // An answer to this request only belongs to the dialog if it is
            // still open and still about this same system: a fetch that was
            // overtaken by a close, or by a reopen on another system, would
            // otherwise answer the new dialog with the old system's facts.
            let still_asking = move || open.get_untracked() && system_id.get_untracked() == id;
            spawn_local(async move {
                let fetched = fetch_system_details(id).await;
                if !still_asking() {
                    return;
                }
                match fetched {
                    Ok(fetched) => details.set(Some(fetched)),
                    Err(e) => report_error(state.notifier, "Loading system information", &e),
                }
            });
        }
    });

    view! {
        <Modal open=open title=Signal::derive(move || title.get()) size="md">
            {move || {
                details.get().map(|details| {
                    let SystemDetails {
                        system:
                            SystemFacts {
                                name,
                                version,
                                games_complete,
                                games_total,
                                completion,
                            },
                        total_original_size,
                        one_region_original_size,
                        total_actual_size,
                        one_region_actual_size,
                        store,
                    } = details;
                    view! {
                        <div class="wa-stack wa-gap-l">
                            <div class="wa-stack wa-gap-3xs">
                                <span class="wa-font-weight-semibold">{name.clone()}</span>
                                <small style="color: var(--wa-color-text-quiet);">
                                    {format!("DAT {}", version)}
                                </small>
                            </div>

                            <div class="wa-stack wa-gap-xs">
                                <InfoRow label="Completion">
                                    {completion_word(completion).to_string()}
                                </InfoRow>
                                <InfoRow label="Games">
                                    {format!("{games_complete} / {games_total}")}
                                </InfoRow>
                                <InfoRow label="Total Size (Original)">
                                    <InfoBytes value=total_original_size/>
                                </InfoRow>
                                <InfoRow label="1G1R Size (Original)">
                                    <InfoBytes value=one_region_original_size/>
                                </InfoRow>
                                <InfoRow label="Total Size (Actual)">
                                    <InfoBytes value=total_actual_size/>
                                </InfoRow>
                                <InfoRow label="1G1R Size (Actual)">
                                    <InfoBytes value=one_region_actual_size/>
                                </InfoRow>
                            </div>

                            {store
                                .map(|store| {
                                    // The other systems in the store's
                                    // group: the badges, and the reason
                                    // the section about them exists.
                                    let shared: Vec<String> = store
                                        .systems
                                        .iter()
                                        .filter(|other| *other != &name)
                                        .cloned()
                                        .collect();
                                    view! {
                                        <>
                                            <wa-divider></wa-divider>
                                            <div class="wa-flank wa-gap-s wa-align-items-center">
                                                <wa-icon name="database"></wa-icon>
                                                <span class="wa-font-weight-semibold">"ORE Store"</span>
                                            </div>
                                            <div class="wa-stack wa-gap-xs">
                                                <InfoRow label="Path"><Mono>{store.path.clone()}</Mono></InfoRow>
                                                <InfoRow label="Store Size"><InfoBytes value=store.size/></InfoRow>
                                                <InfoRow label="Dedup Saved">
                                                    <InfoBytes value=store.dedup_saved/>
                                                </InfoRow>
                                                <InfoRow label="Parity Cost">
                                                    <InfoBytes value=store.parity_cost/>
                                                </InfoRow>
                                                <InfoRow label="This System's Share of Dedup">
                                                    <InfoBytes value=store.system_dedup_saved/>
                                                </InfoRow>
                                                <InfoRow label="This System's Share of Parity">
                                                    <InfoBytes value=store.system_parity_cost/>
                                                </InfoRow>
                                            </div>
                                            {if shared.is_empty() {
                                                None
                                            } else {
                                                Some(
                                                    view! {
                                                        <div class="wa-stack wa-gap-2xs">
                                                            <small
                                                                style="color: var(--wa-color-text-quiet);"
                                                            >
                                                                Shared with
                                                            </small>
                                                            <div class="wa-cluster wa-gap-2xs">
                                                                {shared
                                                                    .into_iter()
                                                                    .map(|other| {
                                                                        view! {
                                                                            <wa-badge
                                                                                appearance="outlined"
                                                                                pill=""
                                                                            >
                                                                                {other}
                                                                            </wa-badge>
                                                                        }
                                                                    })
                                                                    .collect_view()}
                                                            </div>
                                                        </div>
                                                    }
                                                        .into_any(),
                                                )
                                            }}
                                        </>
                                    }
                                })}
                        </div>
                    }
                })
            }}
        </Modal>
    }
}
