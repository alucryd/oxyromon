//! Notifications bell + dropdown (ports `NotificationsButton.svelte`).

use leptos::prelude::*;

use crate::icons::{BELL, Icon};
use crate::state::AppState;

#[component]
pub fn NotificationsButton() -> impl IntoView {
    let state = expect_context::<AppState>();
    let open = RwSignal::new(false);
    let notifications = state.notifications;

    let count = move || notifications.get().len();
    let has_any = move || !notifications.get().is_empty();

    view! {
        <div class="relative">
            <button
                class="relative rounded-lg bg-slate-700 p-2.5 text-white hover:bg-slate-600"
                title="Notifications"
                on:click=move |_| open.update(|o| *o = !*o)
            >
                <Icon path=BELL class="h-5 w-5" />
                <Show when=move || has_any()>
                    <span class="absolute -top-1 -right-1 flex h-4 w-4 items-center justify-center rounded-full bg-rose-500 text-xs font-bold text-white">
                        {move || if count() > 99 { "99+".to_string() } else { count().to_string() }}
                    </span>
                </Show>
            </button>

            <Show when=move || open.get()>
                <div class="fixed inset-0 z-30" on:click=move |_| open.set(false)></div>
                <div class="absolute top-full right-0 z-40 mt-1 flex w-80 flex-col rounded-lg border border-slate-600 bg-slate-800 shadow-xl">
                    <div class="flex items-center justify-between border-b border-slate-600 px-3 py-2">
                        <span class="text-sm font-semibold text-slate-200">Notifications</span>
                        <Show when=move || has_any()>
                            <button
                                class="text-xs text-slate-400 hover:text-slate-200"
                                on:click=move |_| notifications.set(Vec::new())
                            >
                                Clear all
                            </button>
                        </Show>
                    </div>
                    <div class="max-h-80 overflow-y-auto">
                        <Show
                            when=move || has_any()
                            fallback=|| {
                                view! {
                                    <p class="px-3 py-4 text-center text-sm text-slate-400">
                                        No notifications
                                    </p>
                                }
                            }
                        >
                            <For
                                each=move || notifications.get()
                                key=|n| n.id
                                let:notification
                            >
                                <div class="flex items-start gap-2 border-b border-slate-700 px-3 py-2 last:border-0">
                                    <span class=format!(
                                        "mt-1.5 h-2 w-2 shrink-0 rounded-full {}",
                                        notification.kind.dot_class(),
                                    )></span>
                                    <div class="min-w-0 flex-1">
                                        <p class="text-sm text-slate-200">
                                            {notification.message.clone()}
                                        </p>
                                        <p class="text-xs text-slate-500">
                                            {notification.time.clone()}
                                        </p>
                                    </div>
                                </div>
                            </For>
                        </Show>
                    </div>
                </div>
            </Show>
        </div>
    }
}
