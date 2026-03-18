use super::database::*;
use super::mutation::Mutation;
use super::query::{GameLoader, QueryRoot, RomfileLoader, SystemLoader};
use async_graphql::dataloader::DataLoader;
use async_graphql::{EmptySubscription, Schema};
use async_graphql_axum::GraphQL;
use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{Response, StatusCode, header},
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post_service},
    serve,
};
use clap::{Arg, ArgMatches, Command};
use futures::stream::{Stream, StreamExt};
use http_types::Mime;
use http_types::mime::{BYTE_STREAM, HTML};
use rust_embed::RustEmbed;
use serde::Serialize;
use simple_error::SimpleResult;
use sqlx::sqlite::SqlitePool;
use std::convert::Infallible;
use std::path::PathBuf;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::{select, signal};
use tokio_util::io::ReaderStream;
use tokio_util::sync::CancellationToken;
#[cfg(debug_assertions)]
use tower_http::cors::{Any, CorsLayer};

#[derive(RustEmbed)]
#[folder = "target/assets"]
struct Assets;

/// Message structure for Server-Sent Events
///
/// # Example
/// ```
/// use oxyromon::server::SseMessage;
/// use tokio::sync::broadcast;
///
/// let (tx, _rx) = broadcast::channel::<SseMessage>(100);
///
/// // Send a message to all connected SSE clients
/// let _ = tx.send(SseMessage {
///     event: "progress".to_string(),
///     data: serde_json::json!({
///         "current": 50,
///         "total": 100,
///         "message": "Processing..."
///     }).to_string(),
/// });
/// ```
#[derive(Clone, Debug, Serialize)]
pub struct SseMessage {
    pub event: String,
    pub data: String,
}

/// Shared application state
///
/// Contains the database pool and SSE broadcast channel.
/// The `sse_tx` can be used to publish messages to all connected SSE clients.
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub sse_tx: broadcast::Sender<SseMessage>,
    pub cancel: CancellationToken,
}

pub fn subcommand() -> Command {
    Command::new("server")
        .about("Launch the backend server")
        .arg(
            Arg::new("ADDRESS")
                .short('a')
                .long("address")
                .help("Specify the server address")
                .required(false)
                .num_args(1)
                .default_value("127.0.0.1"),
        )
        .arg(
            Arg::new("PORT")
                .short('p')
                .long("port")
                .help("Specify the server port")
                .required(false)
                .num_args(1)
                .default_value("8000"),
        )
}

async fn serve_index() -> Response<Body> {
    Response::builder()
        .header(header::CONTENT_TYPE, HTML.to_string())
        .body(Body::from(Assets::get("index.html").unwrap().data.to_vec()))
        .unwrap()
}

async fn serve_asset(Path(path): Path<String>) -> Response<Body> {
    match Assets::get(&path) {
        Some(file) => {
            let mime = Mime::sniff(file.data.as_ref())
                .or_else(|err| {
                    Mime::from_extension(
                        std::path::Path::new(&path)
                            .extension()
                            .unwrap()
                            .to_str()
                            .unwrap(),
                    )
                    .ok_or(err)
                })
                .unwrap_or(BYTE_STREAM);
            Response::builder()
                .header(header::CONTENT_TYPE, mime.to_string())
                .body(Body::from(file.data.to_vec()))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from(vec![]))
            .unwrap(),
    }
}

async fn shutdown_signal(pool: SqlitePool, cancel: CancellationToken) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    optimize_database(pool).await;
    cancel.cancel();
}

pub async fn main(pool: SqlitePool, matches: &ArgMatches) -> SimpleResult<()> {
    // Create broadcast channel for SSE
    let (sse_tx, _) = broadcast::channel::<SseMessage>(100);
    let cancel = CancellationToken::new();

    let state = AppState {
        pool: pool.clone(),
        sse_tx: sse_tx.clone(),
        cancel: cancel.clone(),
    };

    let schema = Schema::build(QueryRoot, Mutation, EmptySubscription)
        .data(DataLoader::new(
            SystemLoader { pool: pool.clone() },
            tokio::task::spawn,
        ))
        .data(DataLoader::new(
            GameLoader { pool: pool.clone() },
            tokio::task::spawn,
        ))
        .data(DataLoader::new(
            RomfileLoader { pool: pool.clone() },
            tokio::task::spawn,
        ))
        .data(pool.clone())
        .data(sse_tx)
        .finish();

    let app = Router::new()
        .route("/graphql", post_service(GraphQL::new(schema)))
        .route("/events", get(sse_handler))
        .route("/romfiles/{id}", get(download_romfile))
        .route("/{*path}", get(serve_asset))
        .route("/", get(serve_index))
        .with_state(state);

    #[cfg(debug_assertions)]
    let app = {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);
        app.layer(cors)
    };

    let listener = TcpListener::bind(format!(
        "{}:{}",
        matches.get_one::<String>("ADDRESS").unwrap(),
        matches.get_one::<String>("PORT").unwrap()
    ))
    .await
    .unwrap();

    serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal(pool, cancel))
        .await
        .unwrap();

    Ok(())
}

async fn download_romfile(
    Path(id): Path<i64>,
    State(state): State<AppState>,
) -> Response<Body> {
    let mut connection = state.pool.acquire().await.unwrap();

    let rom_directory = match find_setting_by_key(&mut connection, "ROM_DIRECTORY", None).await {
        Some(setting) => match setting.value {
            Some(value) => value,
            None => {
                return Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::empty())
                    .unwrap();
            }
        },
        None => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap();
        }
    };

    let romfile = find_romfile_by_id(&mut connection, id).await;
    let file_path = PathBuf::from(&rom_directory).join(&romfile.path);

    let file = match tokio::fs::File::open(&file_path).await {
        Ok(f) => f,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::empty())
                .unwrap();
        }
    };

    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("download");

    let content_disposition = format!("attachment; filename=\"{}\"", filename);
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Response::builder()
        .header(header::CONTENT_TYPE, BYTE_STREAM.to_string())
        .header(header::CONTENT_DISPOSITION, content_disposition)
        .body(body)
        .unwrap()
}

/// SSE endpoint handler
///
/// Handles Server-Sent Events connections at `/events`.
/// Clients can connect to this endpoint to receive real-time updates.
///
/// # Client Usage (JavaScript/Svelte)
/// ```javascript
/// const eventSource = new EventSource('/events');
///
/// eventSource.addEventListener('progress', (event) => {
///     const data = JSON.parse(event.data);
///     console.log('Progress:', data);
/// });
///
/// eventSource.addEventListener('error', (event) => {
///     console.error('SSE error:', event);
/// });
/// ```
async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.sse_tx.subscribe();
    let cancel = state.cancel.clone();

    let stream = async_stream::stream! {
        loop {
            select! {
                msg = rx.recv() => {
                    match msg {
                        Ok(msg) => {
                            let event = Event::default()
                                .event(msg.event)
                                .data(msg.data);
                            yield Ok(event);
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            log::warn!("SSE client lagged by {} messages", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }
                _ = cancel.cancelled() => {
                    break;
                }
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod test_server;
