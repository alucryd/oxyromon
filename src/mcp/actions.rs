// The shared execution path for every mutating tool: one connection from the
// pool, the process-wide write lock, a hidden progress bar whose movement is
// bridged to MCP `notifications/progress` when the caller passed a
// `progressToken`, per-request cancellation, and SSE fan-out when mounted on
// the web server (so a UI watching `/events` also sees MCP-triggered work).

use super::OxyromonMcp;
use crate::progress::*;
use anyhow::Result;
use rmcp::model::{CallToolResult, ContentBlock, ProgressNotificationParam, ProgressToken};
use rmcp::service::{Peer, RoleServer};
use sqlx::SqliteConnection;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[cfg(feature = "server")]
use crate::server::sse_send;

/// Success result carrying a JSON payload both as `structuredContent` and as
/// text, so any client can read it.
pub(crate) fn ok_json<T: serde::Serialize>(value: &T) -> CallToolResult {
    match serde_json::to_value(value) {
        Ok(value) => CallToolResult::structured(value),
        Err(e) => tool_error(format!("failed to serialize the result: {e}")),
    }
}

/// Success result for JSON-incompatible payloads: the tool ran, here is prose.
pub(crate) fn ok_text(message: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(message.into())])
}

/// The tool ran and did not work — a *tool-level* error, so the client renders
/// the message to the user (as opposed to a protocol `Err(McpError)`, which is
/// for requests the server cannot even route).
pub(crate) fn tool_error(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message.into())])
}

pub(crate) fn need_confirm(tool: &str) -> CallToolResult {
    tool_error(format!(
        "{tool} is destructive and was refused: pass \"confirm\": true to proceed"
    ))
}

pub(crate) fn read_only_denial(tool: &str) -> CallToolResult {
    tool_error(format!(
        "{tool} is not available: this server runs with --read-only"
    ))
}

impl OxyromonMcp {
    /// Run one mutating action to completion. `work` drives the existing
    /// subcommand internals on a pooled connection, reporting progress on the
    /// given bar — the same `BoxFuture` shape `spawn_cli_action` uses in
    /// `src/mutation.rs`, which keeps these closures easy to type.
    pub(crate) async fn run_action<T>(
        &self,
        label: String,
        sse_prefix: Option<&'static str>,
        peer: Peer<RoleServer>,
        progress_token: Option<ProgressToken>,
        ct: CancellationToken,
        work: impl for<'a> FnOnce(
            &'a mut SqliteConnection,
            &'a ProgressBar,
        ) -> futures::future::BoxFuture<'a, Result<T>>,
    ) -> Result<T, String>
    where
        T: Send + 'static,
    {
        // the prefix only feeds the SSE bridge, which exists in server builds
        #[cfg(not(feature = "server"))]
        let _ = sse_prefix;

        // SQLite serializes writers anyway; do it politely instead of hitting
        // SQLITE_BUSY halfway through an import.
        let _guard = self.write_lock.lock().await;

        let mut connection = self
            .pool
            .acquire()
            .await
            .map_err(|e| format!("failed to acquire a database connection: {e}"))?;

        // No TTY in MCP mode: the bar exists so the poller below has something
        // to read, and so the log bridge keeps the styled output on stderr.
        let progress_bar = get_progress_bar(0, get_none_progress_style());
        start_action(&progress_bar, Some(&label));

        #[cfg(feature = "server")]
        if let Some(sse_tx) = &self.sse_tx
            && let Some(prefix) = sse_prefix
        {
            sse_send(
                sse_tx,
                &format!("{prefix}_started"),
                serde_json::json!({ "message": label }),
            );
        }

        // Bridge bar movement to notifications/progress. The subcommands own
        // the bar (they set style, length, message, and tick it); we only read
        // it, ~twice a second, and forward what changed.
        let poller = progress_token.clone().map(|token| {
            let bar = progress_bar.clone();
            tokio::spawn(async move {
                let mut last: (u64, Option<u64>, String) = (0, None, String::new());
                loop {
                    tokio::time::sleep(Duration::from_millis(400)).await;
                    let state = (bar.position(), bar.length(), bar.message());
                    if state == last {
                        continue;
                    }
                    last = state.clone();
                    let mut progress =
                        ProgressNotificationParam::new(token.clone(), state.0 as f64);
                    progress.total = state.1.map(|total| total as f64);
                    if !state.2.is_empty() {
                        progress.message = Some(state.2);
                    }
                    if peer.notify_progress(progress).await.is_err() {
                        break;
                    }
                }
            })
        });

        let result = tokio::select! {
            result = work(&mut connection, &progress_bar) => result,
            _ = ct.cancelled() => Err(anyhow::anyhow!("cancelled by the client")),
            _ = self.cancel.cancelled() => Err(anyhow::anyhow!("server shutting down")),
        };

        if let Some(poller) = poller {
            poller.abort();
        }
        stop_action(&progress_bar);
        progress_bar.finish_and_clear();

        match &result {
            Ok(_) => {
                log::info!("{label}: done");
                #[cfg(feature = "server")]
                if let Some(sse_tx) = &self.sse_tx
                    && let Some(prefix) = sse_prefix
                {
                    sse_send(
                        sse_tx,
                        &format!("{prefix}_complete"),
                        serde_json::json!({ "success": true, "message": format!("{label}: done") }),
                    );
                }
            }
            Err(e) => {
                log::error!("{label}: {e:#}");
                #[cfg(feature = "server")]
                if let Some(sse_tx) = &self.sse_tx
                    && let Some(prefix) = sse_prefix
                {
                    sse_send(
                        sse_tx,
                        &format!("{prefix}_error"),
                        serde_json::json!({
                            "success": false,
                            "message": format!("{label}: {e:#}"),
                        }),
                    );
                }
            }
        }

        result.map_err(|e| format!("{e:#}"))
    }

    /// Notify clients that the resource list changed (systems came and went).
    pub(crate) async fn notify_resources_changed(&self, peer: &Peer<RoleServer>) {
        let _ = peer.notify_resource_list_changed().await;
    }

    /// Notify clients that one resource's content changed.
    pub(crate) async fn notify_resource_updated(&self, peer: &Peer<RoleServer>, uri: &str) {
        let _ = peer
            .notify_resource_updated(rmcp::model::ResourceUpdatedNotificationParam::new(uri))
            .await;
    }
}
