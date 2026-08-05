mod api;
mod app;
mod components;
mod model;
mod notify;
mod page;
mod sse;
mod state;
mod ui;

use leptos::prelude::*;

use crate::app::App;
use crate::components::navbar::init_theme;

fn main() {
    console_error_panic_hook::set_once();
    init_theme();
    mount_to_body(|| view! { <App /> });
}
