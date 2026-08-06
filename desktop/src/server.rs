//! Lifecycle management for the bundled `oxyromon server` sidecar.
//!
//! The desktop shell does not reimplement the backend: it spawns the regular
//! `oxyromon` binary in `server` mode on an ephemeral loopback port and points
//! the webview straight at it. Because the window is then served *from* that
//! origin, the Leptos SPA keeps using same-origin relative URLs for
//! `/graphql`, `/events`, `/dats` and `/romfiles/{id}` — no CORS, and no
//! desktop-specific frontend build.

use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};

/// How long to wait for the sidecar to start accepting connections.
const READY_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Handle to the running sidecar, kept in Tauri's managed state so it can be
/// terminated when the app exits.
#[derive(Default)]
pub struct Server(Mutex<Option<CommandChild>>);

impl Server {
    /// Terminate the sidecar, if it is still running. Safe to call twice.
    pub fn shutdown(&self) {
        if let Some(child) = self.0.lock().unwrap().take()
            && let Err(e) = child.kill()
        {
            log_line(format!("failed to stop server: {e}"));
        }
    }
}

/// Ask the OS for an unused loopback port.
///
/// The listener is dropped before the sidecar binds it, so there is a small
/// race window; in practice the kernel does not hand out the same port twice in
/// quick succession, and a collision merely fails the readiness check below.
fn free_port() -> io::Result<u16> {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))?;
    listener.local_addr().map(|addr| addr.port())
}

/// Block until the sidecar accepts a TCP connection on `port`.
fn wait_until_ready(port: u16) -> Result<(), String> {
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let deadline = Instant::now() + READY_TIMEOUT;
    while Instant::now() < deadline {
        if TcpStream::connect(address).is_ok() {
            return Ok(());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    Err(format!(
        "the oxyromon server did not start listening on port {port} within {}s",
        READY_TIMEOUT.as_secs()
    ))
}

fn log_line(message: impl AsRef<str>) {
    eprintln!("[oxyromon] {}", message.as_ref());
}

/// Spawn the sidecar and wait for it to become reachable.
///
/// Returns the port it is listening on.
pub fn start(app: &AppHandle) -> Result<u16, String> {
    let port = free_port().map_err(|e| format!("failed to reserve a port: {e}"))?;

    let command = app
        .shell()
        .sidecar("oxyromon")
        .map_err(|e| format!("failed to locate the bundled oxyromon binary: {e}"))?
        .args([
            "server",
            "--address",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ]);

    let (mut events, child) = command
        .spawn()
        .map_err(|e| format!("failed to start the oxyromon server: {e}"))?;

    app.state::<Server>().0.lock().unwrap().replace(child);

    // Surface the server's output on the desktop app's stderr for debugging.
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            match event {
                CommandEvent::Stdout(bytes) | CommandEvent::Stderr(bytes) => {
                    log_line(String::from_utf8_lossy(&bytes).trim_end())
                }
                CommandEvent::Error(e) => log_line(format!("server error: {e}")),
                CommandEvent::Terminated(status) => {
                    log_line(format!("server exited with {:?}", status.code))
                }
                _ => {}
            }
        }
    });

    wait_until_ready(port)?;
    Ok(port)
}
