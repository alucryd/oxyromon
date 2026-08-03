//! Small reusable UI building blocks: the modal dialog, and the windowing used
//! to keep long lists cheap to draw.

use leptos::prelude::*;

use crate::icons::{CLOSE_CIRCLE, Icon};
use crate::state::ROW_HEIGHT;

/// Tracks how far a scroll container has been scrolled and how tall it is, so a
/// long list can draw only the rows that are actually on screen.
///
/// Rows are a fixed [`ROW_HEIGHT`], which is what makes the arithmetic — and
/// therefore a correctly sized scrollbar — possible without measuring anything.
#[derive(Clone, Copy)]
pub struct ScrollWindow {
    scroll_top: RwSignal<f64>,
    viewport: RwSignal<f64>,
}

/// Rows to draw above and below the visible range, so scrolling does not expose
/// blank space before the next render lands.
const OVERSCAN: usize = 8;

impl ScrollWindow {
    pub fn new() -> Self {
        Self {
            scroll_top: RwSignal::new(0.0),
            viewport: RwSignal::new(0.0),
        }
    }

    /// Record the geometry of the scroll container. Call from `on:scroll`, and
    /// once the element exists so the first render is not an empty window.
    pub fn measure(&self, element: &web_sys::Element) {
        self.scroll_top.set(element.scroll_top() as f64);
        self.viewport.set(element.client_height() as f64);
    }

    /// Forget how far we were scrolled. The caller resets the element itself,
    /// since the DOM does not report that change back to us.
    pub fn reset(&self) {
        self.scroll_top.set(0.0);
    }

    /// The half-open range of rows to draw out of `total`.
    pub fn range(&self, total: usize) -> (usize, usize) {
        let first = (self.scroll_top.get() / ROW_HEIGHT).floor() as usize;
        let start = first.saturating_sub(OVERSCAN);
        // Before the container has been measured, draw a screenful so there is
        // something on the first paint.
        let visible = if self.viewport.get() > 0.0 {
            (self.viewport.get() / ROW_HEIGHT).ceil() as usize
        } else {
            30
        };
        let end = (first + visible + OVERSCAN).min(total);
        (start, end.max(start))
    }
}

impl Default for ScrollWindow {
    fn default() -> Self {
        Self::new()
    }
}

/// Spacer that stands in for the rows above or below the drawn window, keeping
/// the scrollbar proportional to the whole list.
#[component]
pub fn Spacer(#[prop(into)] rows: Signal<usize>) -> impl IntoView {
    view! { <div style:height=move || format!("{}px", rows.get() as f64 * ROW_HEIGHT)></div> }
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
