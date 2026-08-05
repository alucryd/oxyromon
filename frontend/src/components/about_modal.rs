//! About dialog with build info, stats and dependency badges.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{get_info, report_error};
use crate::model::Info;
use crate::state::AppState;
use crate::ui::Modal;

/// One statistic, as a filled card.
#[component]
fn Stat(label: &'static str, value: Signal<Option<i64>>) -> impl IntoView {
    view! {
        <wa-card appearance="filled-outlined">
            <div class="wa-stack wa-gap-3xs" style="align-items: center;">
                <span style="font-size: var(--wa-font-size-xl); font-weight: var(--wa-font-weight-bold);">
                    {move || value.get().map(|value| value.to_string()).unwrap_or_default()}
                </span>
                <small style="color: var(--wa-color-text-quiet);">{label}</small>
            </div>
        </wa-card>
    }
}

#[component]
pub fn AboutModal() -> impl IntoView {
    let state = expect_context::<AppState>();
    let info = RwSignal::new(Option::<Info>::None);

    // Fetch once, the first time the modal is opened.
    Effect::new(move |_| {
        if state.about_modal_open.get() && info.get_untracked().is_none() {
            spawn_local(async move {
                match get_info().await {
                    Ok(data) => info.set(Some(data)),
                    Err(e) => report_error(state.notifier, "Loading version information", &e),
                }
            });
        }
    });

    view! {
        <Modal
            open=state.about_modal_open
            title=Signal::derive(|| "About oxyROMon".to_string())
            size="sm"
        >
            <div class="wa-stack wa-gap-l">
                <div class="wa-stack wa-gap-2xs" style="align-items: center; text-align: center;">
                    <img src="/logo.svg" alt="oxyROMon" style="height: 3rem;" />
                    <span style="font-weight: var(--wa-font-weight-semibold);">
                        {move || {
                            info.get()
                                .map(|info| format!("oxyROMon {}", info.version))
                                .unwrap_or_default()
                        }}
                    </span>
                    <small style="color: var(--wa-color-text-quiet);">"Rusty ROM OrgaNizer"</small>
                </div>

                <wa-divider></wa-divider>

                <div class="wa-grid wa-gap-s" style="--min-column-size: 6rem;">
                    <Stat label="Systems" value=Signal::derive(move || info.get().map(|info| info.system_count)) />
                    <Stat label="Games" value=Signal::derive(move || info.get().map(|info| info.game_count)) />
                    <Stat label="ROMs" value=Signal::derive(move || info.get().map(|info| info.rom_count)) />
                </div>

                <div class="wa-stack wa-gap-xs">
                    <small style="color: var(--wa-color-text-quiet);">Dependencies</small>
                    <div class="wa-cluster wa-gap-2xs">
                        <For
                            each=move || info.get().map(|info| info.dependencies).unwrap_or_default()
                            key=|dependency| dependency.name.clone()
                            let:dependency
                        >
                            {
                                let found = dependency
                                    .version
                                    .as_deref()
                                    .is_some_and(|version| {
                                        !version.is_empty() && version != "unknown"
                                    });
                                let label = match &dependency.version {
                                    Some(version) if found => {
                                        format!("{} {}", dependency.name, version)
                                    }
                                    _ => dependency.name.clone(),
                                };
                                // Missing tools are the interesting case, so let
                                // those stand out rather than the routine ones.
                                let variant = if dependency.version.is_some() {
                                    "neutral"
                                } else {
                                    "warning"
                                };
                                view! {
                                    <wa-badge variant=variant appearance="outlined" pill="">
                                        {label}
                                    </wa-badge>
                                }
                            }
                        </For>
                    </div>
                </div>

                <wa-divider></wa-divider>

                <small style="color: var(--wa-color-text-quiet);">
                    "If you find oxyROMon useful, please consider "
                    <a href="https://ko-fi.com/alucryd" target="_blank">
                        <wa-icon name="mug-hot"></wa-icon>
                        " buying me a coffee"
                    </a> "."
                </small>
            </div>
        </Modal>
    }
}
