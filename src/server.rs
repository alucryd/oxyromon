use super::database::*;
use super::import_dats::{ImportDatResult, process_dat_upload};
use super::mutation::Mutation;
use super::progress::*;
use super::query::{GameLoader, QueryRoot, RomfileLoader, SystemLoader};
use async_graphql::dataloader::DataLoader;
use async_graphql::{EmptySubscription, Schema};
use async_graphql_axum::GraphQL;
use axum::{
    Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{Response, StatusCode, header},
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post, post_service},
    serve,
};
use clap::{Arg, ArgMatches, Command};
use futures::stream::Stream;
use http_types::Mime;
use http_types::mime::{BYTE_STREAM, HTML};
use rust_embed::RustEmbed;
use serde::Serialize;
use serde_json::json;
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
        .route("/dats", post(upload_dat).layer(DefaultBodyLimit::disable()))
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

async fn download_romfile(Path(id): Path<i64>, State(state): State<AppState>) -> Response<Body> {
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

async fn upload_dat(State(state): State<AppState>, mut multipart: Multipart) -> Response<Body> {
    log::info!("upload_dat: processing upload request");

    let mut filename = None;
    let mut data = None;
    let mut update = false;

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                log::error!("upload_dat: multipart error: {}", e);
                return Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Body::from(e.to_string()))
                    .unwrap();
            }
        };

        // Own name and file_name before consuming the field
        let field_name = field.name().map(|s| s.to_owned());
        let file_name = field.file_name().map(|s| s.to_owned());

        match field_name.as_deref() {
            Some("file") => {
                filename = file_name;
                match field.bytes().await {
                    Ok(bytes) => data = Some(bytes),
                    Err(e) => {
                        log::error!("upload_dat: failed to read file bytes: {}", e);
                        return Response::builder()
                            .status(StatusCode::BAD_REQUEST)
                            .body(Body::from(e.to_string()))
                            .unwrap();
                    }
                }
            }
            Some("update") => match field.text().await {
                Ok(text) => update = text.trim() == "true",
                Err(e) => log::warn!("upload_dat: failed to read update field: {}", e),
            },
            other => {
                log::debug!("upload_dat: skipping unknown field {:?}", other);
                let _ = field.bytes().await;
            }
        }
    }

    let (filename, data) = match (filename, data) {
        (Some(f), Some(d)) => (f, d),
        _ => {
            log::warn!("upload_dat: no file field in request");
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from("No file uploaded"))
                .unwrap();
        }
    };

    log::info!(
        "upload_dat: queuing import for '{}' (update={})",
        filename,
        update
    );

    let sse_tx = state.sse_tx.clone();
    let pool = state.pool.clone();

    tokio::spawn(async move {
        let mut connection = pool.acquire().await.unwrap();
        let progress_bar = ProgressBar::hidden();

        let _ = sse_tx.send(SseMessage {
            event: "import_dat_started".to_string(),
            data: json!({
                "filename": filename,
                "message": format!("Importing '{}'", filename),
            })
            .to_string(),
        });

        let tmp_dir = match tempfile::TempDir::new() {
            Ok(d) => d,
            Err(e) => {
                let _ = sse_tx.send(SseMessage {
                    event: "import_dat_error".to_string(),
                    data: json!({
                        "filename": filename,
                        "error": e.to_string(),
                        "message": format!("Failed to create temp directory: {}", e),
                    })
                    .to_string(),
                });
                return;
            }
        };

        let upload_path = tmp_dir.path().join(&filename);
        if let Err(e) = tokio::fs::write(&upload_path, &data).await {
            let _ = sse_tx.send(SseMessage {
                event: "import_dat_error".to_string(),
                data: json!({
                    "filename": filename,
                    "error": e.to_string(),
                    "message": format!("Failed to save uploaded file: {}", e),
                })
                .to_string(),
            });
            return;
        }

        let results =
            process_dat_upload(&mut connection, &progress_bar, &upload_path, update).await;

        for result in results {
            match result {
                Ok(ImportDatResult::Imported(summary)) => {
                    let _ = sse_tx.send(SseMessage {
                        event: "import_dat_complete".to_string(),
                        data: json!({
                            "system_name": summary.system_name,
                            "system_version": summary.system_version,
                            "game_count": summary.game_count,
                            "message": format!("Imported '{}' ({} games)", summary.system_name, summary.game_count),
                        })
                        .to_string(),
                    });
                }
                Ok(ImportDatResult::UpToDate(system_name)) => {
                    let _ = sse_tx.send(SseMessage {
                        event: "import_dat_complete".to_string(),
                        data: json!({
                            "skipped": true,
                            "message": format!("'{}' is already up to date", system_name),
                        })
                        .to_string(),
                    });
                }
                Ok(ImportDatResult::Skipped) => {}
                Err(e) => {
                    let _ = sse_tx.send(SseMessage {
                        event: "import_dat_error".to_string(),
                        data: json!({
                            "filename": filename,
                            "error": e.to_string(),
                            "message": format!("Failed to import '{}': {}", filename, e),
                        })
                        .to_string(),
                    });
                }
            }
        }
    });

    Response::builder()
        .status(StatusCode::ACCEPTED)
        .body(Body::empty())
        .unwrap()
}

/// SSE endpoint handler
///
/// Handles Server-Sent Events connections at `/events`.
/// Clients can connect to this endpoint to receive real-time updates.
///
/// # Client Usage (JavaScript)
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
