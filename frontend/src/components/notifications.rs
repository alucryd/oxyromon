//! Notifications bell + dropdown (ports `NotificationsButton.svelte`).

use leptos::prelude::*;

use crate::state::AppState;

#[component]
pub fn NotificationsButton() -> impl IntoView {
    let state = expect_context::<AppState>();
    let open = RwSignal::new(false);
    let notifications = state.notifier.notifications;

    let count = move || notifications.get().len();
    let has_any = move || !notifications.get().is_empty();

    view! {
        <div style="position: relative;">
            <wa-button
                appearance="plain"
                title="Notifications"
                on:click=move |_| open.update(|o| *o = !*o)
            >
                <wa-icon name="bell" label="Notifications"></wa-icon>
                <Show when=move || has_any()>
                    <wa-badge
                        variant="danger"
                        pill=""
                        style="position: absolute; inset-block-start: 0; inset-inline-end: 0;"
                    >
                        {move || if count() > 99 { "99+".to_string() } else { count().to_string() }}
                    </wa-badge>
                </Show>
            </wa-button>

            <Show when=move || open.get()>
                // Closes when the click lands anywhere else.
                <div class="overlay" on:click=move |_| open.set(false)></div>
                <div
                    class="menu"
                    style="top: 100%; inset-inline-end: 0; width: 20rem;"
                >
                    <div class="panel-header">
                        <span>Notifications</span>
                        <Show when=move || has_any()>
                            <wa-button
                                appearance="plain"
                                size="small"
                                on:click=move |_| notifications.set(Vec::new())
                            >
                                Clear all
                            </wa-button>
                        </Show>
                    </div>
                    <div style="max-height: 20rem; overflow-y: auto;">
                        <Show
                            when=move || has_any()
                            fallback=|| {
                                view! {
                                    <p style="padding: var(--wa-space-m); text-align: center; color: var(--wa-color-text-quiet);">
                                        No notifications
                                    </p>
                                }
                            }
                        >
                            <For each=move || notifications.get() key=|n| n.id let:notification>
                                <div
                                    class="wa-flank wa-gap-2xs"
                                    style="align-items: start; padding: var(--wa-space-2xs) var(--wa-space-s); border-block-end: 1px solid var(--wa-color-surface-border);"
                                >
                                    <wa-icon
                                        name=notification.kind.icon()
                                        style=format!(
                                            "color: var(--wa-color-{}-fill-loud); margin-block-start: 0.25rem;",
                                            notification.kind.variant(),
                                        )
                                    ></wa-icon>
                                    <div class="wa-stack wa-gap-3xs">
                                        <span>{notification.message.clone()}</span>
                                        <small style="color: var(--wa-color-text-quiet);">
                                            {notification.time.clone()}
                                        </small>
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
