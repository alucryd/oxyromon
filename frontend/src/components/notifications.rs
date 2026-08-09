//! Notifications bell + popover.

use leptos::prelude::*;

use crate::state::AppState;

/// Ties the popover to its trigger. Only one bell exists, in the navbar.
const TRIGGER_ID: &str = "notifications-trigger";

#[component]
pub fn NotificationsButton() -> impl IntoView {
    let state = expect_context::<AppState>();
    let notifications = state.notifier.notifications;

    let count = move || notifications.get().len();
    let has_any = move || !notifications.get().is_empty();

    view! {
        <div class="notifications">
            <wa-button id=TRIGGER_ID appearance="plain" title="Notifications">
                <wa-icon name="bell" label="Notifications"></wa-icon>
            </wa-button>
            <Show when=move || has_any()>
                <wa-badge class="notifications-count" variant="danger" pill="">
                    {move || if count() > 99 { "99+".to_string() } else { count().to_string() }}
                </wa-badge>
            </Show>
        </div>

        // Anchoring to the trigger by id lets the popover open, close on an
        // outside click and position itself. It also renders in the top layer,
        // which is what keeps it out of the navbar's `overflow-x: auto` clip.
        <wa-popover
            class="notifications-popover"
            for=TRIGGER_ID
            placement="bottom-end"
            without-arrow=""
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
            <div class="notifications-list">
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
                        <div class="notification">
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
                                    // `sync` keeps it counting up while the
                                    // panel is open, so a list left on screen
                                    // does not go stale.
                                    <wa-relative-time
                                        date=notification.time.clone()
                                        sync=""
                                    ></wa-relative-time>
                                </small>
                            </div>
                        </div>
                    </For>
                </Show>
            </div>
        </wa-popover>
    }
}
