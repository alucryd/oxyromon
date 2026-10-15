// oxyromon as a Model Context Protocol server, built on the official Rust SDK
// (rmcp): the library's catalog and actions as MCP tools, resources, and
// prompts.
//
// Two transports:
//
// * stdio (`oxyromon mcp`) — the standard way an MCP client launches a local
//   server as a child process. **stdout is the JSON-RPC channel**: nothing
//   else may ever write to it. Progress bars and logs go to stderr (their
//   default), and `src/import_roms.rs` keeps a guard comment where the one
//   stray `println!` used to live.
// * Streamable HTTP — `mcp_http_service()` returns the Tower service the web
//   server nests at `/mcp`; see `src/server.rs`.
//
// Tools reuse the CLI subcommands the same way the GraphQL mutations do — a
// synthesized `ArgMatches` fed to the subcommand's own `main()` — so all three
// surfaces (CLI, GraphQL, MCP) share one implementation per action. Mutating
// tools hold a process-wide write lock: SQLite serializes writers anyway, and
// this makes that explicit instead of surfacing as `SQLITE_BUSY`.

mod actions;
mod output;
mod params;
mod prompts;
mod read;
mod resources;
mod write;

use anyhow::Result;
use clap::{Arg, ArgAction, ArgMatches, Command};
use rmcp::ErrorData as McpError;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{
    Implementation, ListResourceTemplatesResult, ListResourcesResult, PaginatedRequestParams,
    ReadResourceRequestParams, ReadResourceResponse, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::service::RoleServer;
use rmcp::{ServerHandler, ServiceExt, prompt_handler, tool_handler};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[cfg(feature = "server")]
use crate::server::SseMessage;

const INSTRUCTIONS: &str = concat!(
    "oxyromon organizes a ROM library: systems and games come from catalog DATs, ",
    "ROM files on disk are matched against them by hash. \
     Explore with status, list_systems, get_system, list_games, search_games, get_game. \
     IDs always come from list_systems / list_games. \
     Changes run synchronously and stream progress: import_dats, download_dats, import_roms, \
     sort_roms, check_roms, convert_roms, set_setting. \
     Destructive tools (purge_roms, purge_systems, purge_irds) need confirm: true. \
     In read-only mode the mutating tools are not registered at all.",
);

/// The MCP server: a cheap-to-clone handle to the pool plus the locks and
/// flags shared by every tool call.
#[derive(Clone)]
pub struct OxyromonMcp {
    pub(crate) pool: SqlitePool,
    /// Cancelled when the process should stop; in-flight actions bail out.
    pub(crate) cancel: CancellationToken,
    /// SQLite serializes writers; this makes it explicit and ordered instead
    /// of surfacing as SQLITE_BUSY. Held across whole mutating actions.
    pub(crate) write_lock: Arc<Mutex<()>>,
    read_only: bool,
    /// When mounted on the web server, mutating tools fan their outcome into
    /// the SSE stream so the UI reacts to MCP-triggered actions too.
    #[cfg(feature = "server")]
    pub(crate) sse_tx: Option<tokio::sync::broadcast::Sender<SseMessage>>,
}

impl OxyromonMcp {
    fn new(
        pool: SqlitePool,
        cancel: CancellationToken,
        write_lock: Arc<Mutex<()>>,
        read_only: bool,
    ) -> Self {
        OxyromonMcp {
            pool,
            cancel,
            write_lock,
            read_only,
            #[cfg(feature = "server")]
            sse_tx: None,
        }
    }

    /// The live tool router: the read and write routers merged, with the write
    /// tools fully disabled in read-only mode (hidden from `tools/list` and
    /// rejected by `tools/call` — the client never sees them).
    fn tool_router(&self) -> ToolRouter<Self> {
        let write_router = Self::write_router();
        let mut router = Self::read_router();
        if self.read_only {
            for tool in write_router.list_all() {
                router.disable_route(tool.name.to_string());
            }
        }
        router.merge(write_router);
        router
    }
}

/// Any failure the client cannot act on, as a JSON-RPC internal error.
pub(crate) fn internal(message: impl std::fmt::Display) -> McpError {
    McpError::internal_error(message.to_string(), None)
}

pub fn subcommand() -> Command {
    Command::new("mcp")
        .about("Serve the Model Context Protocol on stdin/stdout")
        .arg(
            Arg::new("READ_ONLY")
                .long("read-only")
                .help("Expose only non-mutating tools")
                .required(false)
                .action(ArgAction::SetTrue),
        )
}

/// Serve MCP over stdio until the client closes stdin or the process is
/// asked to quit (Ctrl+C / SIGTERM).
pub async fn main(pool: SqlitePool, matches: &ArgMatches) -> Result<()> {
    let read_only = matches.get_flag("READ_ONLY");
    let cancel = CancellationToken::new();

    let service = OxyromonMcp::new(
        pool.clone(),
        cancel.clone(),
        Arc::new(Mutex::new(())),
        read_only,
    );
    // stdout becomes the protocol channel here; keep every log line on stderr.
    let server = service.serve(rmcp::transport::stdio()).await?;

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = server.waiting() => (),
        // the token unwinds any in-flight mutating action; dropping the
        // running service stops the transport
        _ = tokio::signal::ctrl_c() => cancel.cancel(),
        _ = terminate => cancel.cancel(),
    }

    Ok(())
}

/// The Streamable HTTP service, for the web server to nest at `/mcp`.
///
/// rmcp serves protocol `2026-07-28` clients statelessly (no session id), so
/// the service factory runs per request — all state lives in the cloneable
/// handler, nothing per-session.
#[cfg(feature = "server")]
pub fn mcp_http_service(
    pool: SqlitePool,
    cancel: CancellationToken,
    sse_tx: tokio::sync::broadcast::Sender<SseMessage>,
) -> rmcp::transport::streamable_http_server::tower::StreamableHttpService<
    OxyromonMcp,
    rmcp::transport::streamable_http_server::session::local::LocalSessionManager,
> {
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::streamable_http_server::tower::StreamableHttpService;

    let manager = Arc::new(LocalSessionManager::default());
    // the factory runs per request: share one lock so concurrent requests
    // still serialize their writes
    let write_lock = Arc::new(Mutex::new(()));
    StreamableHttpService::new(
        move || {
            let service = OxyromonMcp::new(pool.clone(), cancel.clone(), write_lock.clone(), false);
            Ok(service.with_sse(sse_tx.clone()))
        },
        manager,
        Default::default(),
    )
}

#[cfg(feature = "server")]
impl OxyromonMcp {
    fn with_sse(mut self, sse_tx: tokio::sync::broadcast::Sender<SseMessage>) -> Self {
        self.sse_tx = Some(sse_tx);
        self
    }
}

#[tool_handler(router = self.tool_router())]
#[prompt_handler]
impl ServerHandler for OxyromonMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new("oxyromon", env!("CARGO_PKG_VERSION")))
        .with_instructions(INSTRUCTIONS)
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListResourcesResult, McpError> {
        resources::list_resources(self, request, context).await
    }

    async fn list_resource_templates(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListResourceTemplatesResult, McpError> {
        resources::list_resource_templates(self, request, context).await
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> std::result::Result<ReadResourceResponse, McpError> {
        resources::read_resource(self, request, context).await
    }
}
