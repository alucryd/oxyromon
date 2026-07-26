//! Small reusable UI building blocks that replace the handful of
//! flowbite-svelte components the app relied on (pagination + modal).

use leptos::prelude::*;

use crate::icons::{
    CHEVRON_DOUBLE_LEFT, CHEVRON_DOUBLE_RIGHT, CHEVRON_LEFT, CHEVRON_RIGHT, CLOSE_CIRCLE, Icon,
};

/// Shared "alternative" (outline) button styling used by pagination controls.
pub const ALT_BUTTON: &str = "flex items-center justify-center rounded-lg border border-gray-300 bg-white px-2 py-1 text-gray-700 hover:bg-gray-100 disabled:cursor-not-allowed disabled:opacity-50 dark:border-gray-600 dark:bg-gray-800 dark:text-gray-300 dark:hover:bg-gray-700";

/// First / prev / page-indicator / next / last pagination bar.
#[component]
pub fn Pagination(page: RwSignal<usize>, total_pages: RwSignal<usize>) -> impl IntoView {
    let first = move || page.get() <= 1;
    let last = move || page.get() >= total_pages.get();
    view! {
        <div class="m-4 mt-auto flex items-center justify-center gap-2">
            <button class=ALT_BUTTON disabled=first on:click=move |_| page.set(1)>
                <Icon path=CHEVRON_DOUBLE_LEFT class="h-4 w-4" />
            </button>
            <button
                class=ALT_BUTTON
                disabled=first
                on:click=move |_| page.update(|n| *n = n.saturating_sub(1).max(1))
            >
                <Icon path=CHEVRON_LEFT class="h-4 w-4" />
            </button>
            <span class="w-full px-3 py-1 text-center text-sm text-slate-600 dark:text-slate-400">
                {move || format!("{} / {}", page.get(), total_pages.get())}
            </span>
            <button
                class=ALT_BUTTON
                disabled=last
                on:click=move |_| {
                    let total = total_pages.get();
                    page.update(|n| *n = (*n + 1).min(total));
                }
            >
                <Icon path=CHEVRON_RIGHT class="h-4 w-4" />
            </button>
            <button class=ALT_BUTTON disabled=last on:click=move |_| page.set(total_pages.get())>
                <Icon path=CHEVRON_DOUBLE_RIGHT class="h-4 w-4" />
            </button>
        </div>
    }
}

fn size_class(size: &str) -> &'static str {
    match size {
        "xs" => "max-w-sm",
        "sm" => "max-w-md",
        "md" => "max-w-lg",
        "lg" => "max-w-2xl",
        _ => "max-w-4xl", // xl and default
    }
}

/// Centered modal dialog with a backdrop. Closes on backdrop click and the
/// header close button. `title` may be empty to omit the header text.
#[component]
pub fn Modal(
    open: RwSignal<bool>,
    #[prop(into)] title: Signal<String>,
    #[prop(optional, into)] size: String,
    children: ChildrenFn,
) -> impl IntoView {
    let panel_class = format!(
        "relative max-h-[90vh] w-full overflow-y-auto rounded-lg bg-white p-6 text-slate-700 shadow-xl dark:bg-gray-800 dark:text-slate-200 {}",
        size_class(&size)
    );
    view! {
        <Show when=move || open.get()>
            <div
                class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
                on:click=move |_| open.set(false)
            >
                <div class=panel_class.clone() on:click=|ev| ev.stop_propagation()>
                    <div class="mb-4 flex items-center justify-between">
                        <h3 class="text-xl font-semibold">{move || title.get()}</h3>
                        <button
                            class="ml-auto rounded-lg p-1.5 text-gray-400 hover:bg-gray-200 hover:text-gray-900 dark:hover:bg-gray-600 dark:hover:text-white"
                            on:click=move |_| open.set(false)
                        >
                            <Icon path=CLOSE_CIRCLE class="h-5 w-5" />
                        </button>
                    </div>
                    {children()}
                </div>
            </div>
        </Show>
    }
}
