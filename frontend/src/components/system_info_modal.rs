//! System info modal: the DAT it was imported from, what it holds and
//! weighs and how complete it is, and — when its games live in an ORE
//! store — what that store weighs, saves and costs, this system's part
//! of it, and the systems it shares it with.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{fetch_system_details, report_error};
use crate::components::info::{InfoBytes, InfoPending, InfoRow, Mono, completion_word};
use crate::model::{SystemDetails, SystemFacts};
use crate::state::AppState;
use crate::ui::{Modal, SizeTile};

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
    let failed = RwSignal::new(false);

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
            failed.set(false);
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
                    Err(e) => {
                        failed.set(true);
                        report_error(state.notifier, "Loading system information", &e);
                    }
                }
            });
        }
    });

    view! {
        <Modal open=open title=Signal::derive(move || title.get()) size="md">
            {move || {
                let Some(details) = details.get() else {
                    return view! { <InfoPending failed=failed/> }.into_any();
                };
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
                let percent = if games_total > 0 {
                    games_complete as f64 * 100.0 / games_total as f64
                } else {
                    0.0
                };
                view! {
                    <div class="wa-stack wa-gap-l">
                        <div class="wa-stack wa-gap-3xs">
                            <span class="wa-font-weight-semibold">{name.clone()}</span>
                            <small style="color: var(--wa-color-text-quiet);">
                                {format!("DAT {}", version)}
                            </small>
                        </div>

                        <div class="wa-stack wa-gap-2xs">
                            <div class="wa-split">
                                <small class="wa-font-weight-semibold">
                                    {completion_word(completion)}
                                </small>
                                <small style="color: var(--wa-color-text-quiet);">
                                    {format!("{games_complete} / {games_total} games")}
                                </small>
                            </div>
                            <wa-progress-bar prop:value=percent></wa-progress-bar>
                        </div>

                        <div class="wa-grid wa-gap-s" style="--min-column-size: 10rem;">
                            <SizeTile label="Total Size (Original)" bytes=total_original_size/>
                            <SizeTile label="1G1R Size (Original)" bytes=one_region_original_size/>
                            <SizeTile label="Total Size (Actual)" bytes=total_actual_size/>
                            <SizeTile label="1G1R Size (Actual)" bytes=one_region_actual_size/>
                        </div>

                        {store
                            .map(|store| {
                                // The other systems in the store's group:
                                // the reason the store is not this
                                // system's alone.
                                let shared: Vec<String> = store
                                    .systems
                                    .iter()
                                    .filter(|other| *other != &name)
                                    .cloned()
                                    .collect();
                                view! {
                                    <wa-card>
                                        <div slot="header" class="wa-flank wa-gap-s wa-align-items-center">
                                            <wa-icon name="database"></wa-icon>
                                            <span class="wa-font-weight-semibold">"ORE Store"</span>
                                        </div>
                                        <div class="wa-stack wa-gap-m">
                                            <Mono>{store.path.clone()}</Mono>
                                            <div class="wa-grid wa-gap-s" style="--min-column-size: 8rem;">
                                                <SizeTile label="Store Size" bytes=store.size/>
                                                <SizeTile label="Dedup Saved" bytes=store.dedup_saved/>
                                                <SizeTile label="Parity Cost" bytes=store.parity_cost/>
                                            </div>
                                            <div class="wa-stack wa-gap-xs">
                                                <InfoRow label="This System's Share of Dedup">
                                                    <InfoBytes value=store.system_dedup_saved/>
                                                </InfoRow>
                                                <InfoRow label="This System's Share of Parity">
                                                    <InfoBytes value=store.system_parity_cost/>
                                                </InfoRow>
                                            </div>
                                            {(!shared.is_empty()).then(|| {
                                                view! {
                                                    <div class="wa-stack wa-gap-2xs">
                                                        <small style="color: var(--wa-color-text-quiet);">
                                                            "Shared with"
                                                        </small>
                                                        <div class="wa-cluster wa-gap-2xs">
                                                            {shared
                                                                .into_iter()
                                                                .map(|other| {
                                                                    view! {
                                                                        <wa-badge appearance="outlined" pill="">
                                                                            {other}
                                                                        </wa-badge>
                                                                    }
                                                                })
                                                                .collect_view()}
                                                        </div>
                                                    </div>
                                                }
                                            })}
                                        </div>
                                    </wa-card>
                                }
                            })}
                    </div>
                }
                    .into_any()
            }}
        </Modal>
    }
}
