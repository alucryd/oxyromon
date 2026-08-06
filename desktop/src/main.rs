// Do not pop up a console window alongside the GUI on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod server;

use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

use crate::server::Server;

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(Server::default())
        .setup(|app| {
            let handle = app.handle();
            let port = server::start(handle)?;

            let url = format!("http://127.0.0.1:{port}")
                .parse()
                .expect("loopback url should always parse");

            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("oxyROMon")
                .inner_size(1400.0, 900.0)
                .min_inner_size(800.0, 600.0)
                .build()?;

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to start oxyromon-desktop");

    app.run(|handle, event| {
        // The sidecar is not in our process group, so stop it explicitly rather
        // than leaving an orphaned server holding the database lock.
        if matches!(event, RunEvent::Exit) {
            handle.state::<Server>().shutdown();
        }
    });
}
