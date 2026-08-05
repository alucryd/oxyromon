//! Small reusable UI building blocks: the modal dialog, and the windowing used
//! to keep long lists cheap to draw.

use leptos::prelude::*;
use wasm_bindgen::JsValue;

/// Read a `value` property off the element that raised an event.
///
/// Web Awesome's form controls re-dispatch the native `input` and `change`
/// events from the custom element itself, so the target is a `<wa-*>` rather
/// than the `<input>` inside its shadow root — which is why `event_target_value`
/// comes back empty for them.
pub fn control_value(event: &web_sys::Event) -> String {
    event
        .target()
        .and_then(|target| js_sys::Reflect::get(&target, &JsValue::from_str("value")).ok())
        .and_then(|value| value.as_string())
        .unwrap_or_default()
}

/// As [`control_value`], for the `checked` property of a switch or checkbox.
pub fn control_checked(event: &web_sys::Event) -> bool {
    event
        .target()
        .and_then(|target| js_sys::Reflect::get(&target, &JsValue::from_str("checked")).ok())
        .and_then(|checked| checked.as_bool())
        .unwrap_or(false)
}

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

/// Preferred width, as the `--width` custom property `<wa-dialog>` reads.
fn dialog_width(size: &str) -> &'static str {
    match size {
        "xs" => "24rem",
        "sm" => "28rem",
        "md" => "32rem",
        "lg" => "42rem",
        _ => "56rem", // xl and default
    }
}

/// Modal dialog.
///
/// A thin wrapper over `<wa-dialog>`, which brings the focus trap, the Escape
/// handling and the labelling that the hand rolled version never had.
///
/// `open` is driven as a property rather than an attribute so the element sees
/// it the moment it changes, and the dialog reports its own closing back
/// through `wa-after-hide` — that is what catches Escape and the close button,
/// which the dialog handles without asking us.
#[component]
pub fn Modal(
    open: RwSignal<bool>,
    #[prop(into)] title: Signal<String>,
    #[prop(optional, into)] size: String,
    children: ChildrenFn,
) -> impl IntoView {
    let width = format!("--width: {}", dialog_width(&size));
    view! {
        <wa-dialog
            label=move || title.get()
            prop:open=move || open.get()
            light-dismiss=""
            style=width
            on:wa-after-hide=move |_: web_sys::Event| open.set(false)
        >
            // Only build the contents while the dialog is actually open. Until
            // the Web Awesome script has upgraded it, `<wa-dialog>` is just an
            // unknown inline element, so anything inside a closed one would
            // spill onto the page for as long as that takes.
            <Show when=move || open.get()>{children()}</Show>
        </wa-dialog>
    }
}
