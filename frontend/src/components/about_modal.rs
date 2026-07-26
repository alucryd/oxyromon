//! About dialog with build info, stats and dependency badges
//! (ports `AboutModal.svelte`).

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::get_info;
use crate::icons::{Icon, MUG_HOT};
use crate::model::Info;
use crate::state::AppState;
use crate::ui::Modal;

#[component]
pub fn AboutModal() -> impl IntoView {
    let state = expect_context::<AppState>();
    let info = RwSignal::new(Option::<Info>::None);

    // Fetch once, the first time the modal is opened.
    Effect::new(move |_| {
        if state.about_modal_open.get() && info.get_untracked().is_none() {
            spawn_local(async move {
                if let Ok(data) = get_info().await {
                    info.set(Some(data));
                }
            });
        }
    });

    view! {
        <Modal
            open=state.about_modal_open
            title=Signal::derive(|| "About oxyROMon".to_string())
            size="md"
        >
            <div class="space-y-4 text-start">
                <div class="flex flex-col items-center gap-2 pb-2">
                    <div class="rounded-xl bg-slate-800 p-3">
                        <img src="/logo.svg" alt="logo" style="height: 48px;" />
                    </div>
                    <Show when=move || info.get().is_some()>
                        <p class="text-lg font-semibold">
                            "oxyROMon " {move || info.get().map(|i| i.version).unwrap_or_default()}
                        </p>
                    </Show>
                    <p class="text-sm text-gray-500 dark:text-gray-400">"Rusty ROM OrgaNizer"</p>
                </div>

                <h6 class="text-sm font-medium text-gray-500 uppercase dark:text-gray-400">
                    Statistics
                </h6>
                <Show
                    when=move || info.get().is_some()
                    fallback=|| {
                        view! {
                            <p class="text-sm text-gray-500 dark:text-gray-400">Loading...</p>
                        }
                    }
                >
                    {move || {
                        let i = info.get().unwrap();
                        view! {
                            <div class="grid grid-cols-3 gap-2 text-center">
                                <div class="rounded bg-gray-100 p-2 dark:bg-gray-700">
                                    <p class="text-lg font-bold">{i.system_count}</p>
                                    <p class="text-xs text-gray-500 dark:text-gray-400">Systems</p>
                                </div>
                                <div class="rounded bg-gray-100 p-2 dark:bg-gray-700">
                                    <p class="text-lg font-bold">{i.game_count}</p>
                                    <p class="text-xs text-gray-500 dark:text-gray-400">Games</p>
                                </div>
                                <div class="rounded bg-gray-100 p-2 dark:bg-gray-700">
                                    <p class="text-lg font-bold">{i.rom_count}</p>
                                    <p class="text-xs text-gray-500 dark:text-gray-400">ROMs</p>
                                </div>
                            </div>
                        }
                    }}
                </Show>

                <h6 class="text-sm font-medium text-gray-500 uppercase dark:text-gray-400">
                    Dependencies
                </h6>
                <Show
                    when=move || info.get().is_some()
                    fallback=|| {
                        view! {
                            <p class="text-sm text-gray-500 dark:text-gray-400">Loading...</p>
                        }
                    }
                >
                    <div class="flex flex-wrap gap-2">
                        <For
                            each=move || info.get().map(|i| i.dependencies).unwrap_or_default()
                            key=|dep| dep.name.clone()
                            let:dep
                        >
                            {
                                let present = dep
                                    .version
                                    .as_deref()
                                    .is_some_and(|v| !v.is_empty() && v != "unknown");
                                let label = match &dep.version {
                                    Some(v) if present => format!("{} {}", dep.name, v),
                                    _ => dep.name.clone(),
                                };
                                let color = if dep.version.is_some() {
                                    "bg-emerald-100 text-emerald-800 dark:bg-emerald-900 dark:text-emerald-300"
                                } else {
                                    "bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-300"
                                };
                                view! {
                                    <span class=format!(
                                        "rounded-lg px-2.5 py-1 text-sm font-medium {color}",
                                    )>{label}</span>
                                }
                            }
                        </For>
                    </div>
                </Show>

                <div class="border-t border-gray-200 pt-4 dark:border-gray-600">
                    <p class="text-sm text-gray-500 dark:text-gray-400">
                        "If you find oxyROMon useful, please consider "
                        <a
                            href="https://ko-fi.com/alucryd"
                            target="_blank"
                            class="inline-flex items-center gap-1 text-primary-600 hover:underline"
                        >
                            <Icon path=MUG_HOT class="h-4 w-4" />
                            "buying me a coffee"
                        </a> "."
                    </p>
                </div>
            </div>
        </Modal>
    }
}
