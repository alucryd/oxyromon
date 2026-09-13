use super::database::*;
use super::import_dats::{ImportDatResult, process_dat_upload};
use super::import_irds;
use super::import_patches::{import_patch, parse_patch};
use super::mutation::Mutation;
use super::progress::*;
use super::query::{GameLoader, QueryRoot, RomfileLoader, SystemLoader};
use anyhow::{bail, Result};
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
use serde_json::{json, Value};
use sqlx::sqlite::SqlitePool;
use std::convert::Infallible;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
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

/// Broadcast a message to every connected SSE client, ignoring a full or closed channel.
pub fn sse_send(tx: &broadcast::Sender<SseMessage>, event: &str, data: Value) {
    let _ = tx.send(SseMessage {
        event: event.to_string(),
        data: data.to_string(),
    });
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

pub async fn main(pool: SqlitePool, matches: &ArgMatches) -> Result<()> {
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
            SystemLoader::new(pool.clone()),
            tokio::task::spawn,
        ))
        .data(DataLoader::new(
            GameLoader::new(pool.clone()),
            tokio::task::spawn,
        ))
        .data(DataLoader::new(
            RomfileLoader::new(pool.clone()),
            tokio::task::spawn,
        ))
        .data(pool.clone())
        .data(sse_tx)
        .finish();

    let app = Router::new()
        .route("/graphql", post_service(GraphQL::new(schema)))
        .route("/events", get(sse_handler))
        .route("/dats", post(upload_dat).layer(DefaultBodyLimit::disable()))
        .route("/roms", post(upload_rom).layer(DefaultBodyLimit::disable()))
        .route("/patches", post(upload_patch).layer(DefaultBodyLimit::disable()))
        .route("/irds", post(upload_ird).layer(DefaultBodyLimit::disable()))
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

/// Import a ROM, either uploaded or fetched from a URL.
///
/// Takes multipart with a `file` field or a `url` field, and an optional
/// `system` to narrow the search the way `import-roms -s` does. Returns as soon
/// as the work is queued; progress arrives over SSE.
///
/// Note the URL is fetched by the server, not the browser, which is what makes
/// it useful — the file never travels via the client — and also means it can
/// reach anything the server can. That is fine for the loopback default; think
/// twice before exposing this beyond it.
async fn upload_rom(State(state): State<AppState>, mut multipart: Multipart) -> Response<Body> {
    let mut upload = None;
    let mut url = None;
    let mut system = None;
    let mut unattended = None;

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(e) => {
                log::error!("upload_rom: multipart error: {}", e);
                return bad_request(e.to_string());
            }
        };

        let field_name = field.name().map(|name| name.to_owned());
        let file_name = field.file_name().map(|name| name.to_owned());

        match field_name.as_deref() {
            Some("file") => {
                // Straight to disk, a chunk at a time: ROMs run to gigabytes, and
                // holding one in memory only to write it out afterwards is one
                // copy of a disc image too many.
                let filename = safe_filename(file_name.as_deref().unwrap_or_default());
                match stream_field_to_directory(field, &filename).await {
                    Ok(directory) => upload = Some((filename, directory)),
                    Err(e) => {
                        log::error!("upload_rom: failed to store the upload: {:#}", e);
                        return bad_request(e.to_string());
                    }
                }
            }
            Some("url") => match field.text().await {
                Ok(text) if !text.trim().is_empty() => url = Some(text.trim().to_owned()),
                Ok(_) => {}
                Err(e) => log::warn!("upload_rom: failed to read url field: {}", e),
            },
            Some("system") => match field.text().await {
                Ok(text) if !text.trim().is_empty() => system = Some(text.trim().to_owned()),
                Ok(_) => {}
                Err(e) => log::warn!("upload_rom: failed to read system field: {}", e),
            },
            Some("unattended") => match field.text().await {
                Ok(text) if !text.trim().is_empty() => {
                    unattended = Some(text.trim().to_owned());
                }
                Ok(_) => {}
                Err(e) => log::warn!("upload_rom: failed to read unattended field: {}", e),
            },
            other => {
                log::debug!("upload_rom: skipping unknown field {:?}", other);
                let _ = field.bytes().await;
            }
        }
    }

    let source = match (upload, url) {
        (Some((filename, directory)), _) => RomSource::Upload {
            filename,
            directory,
        },
        (_, Some(url)) => RomSource::Url(url),
        _ => return bad_request("No file or url provided".to_string()),
    };

    let sse_tx = state.sse_tx.clone();
    let pool = state.pool.clone();

    tokio::spawn(async move {
        let mut connection = pool.acquire().await.unwrap();
        let progress_bar = ProgressBar::hidden();
        let label = source.label();

        sse_send(
            &sse_tx,
            "import_rom_started",
            json!({
                "name": label,
                "message": format!("Importing \"{}\"", label),
            }),
        );

        match import_rom_source(&mut connection, &progress_bar, source, system.as_deref(), unattended.as_deref()).await {
            Ok(()) => {
                sse_send(
                    &sse_tx,
                    "import_rom_complete",
                    json!({
                        "name": label,
                        "success": true,
                        "message": format!("Imported \"{}\"", label),
                    }),
                );
            }
            Err(e) => {
                log::error!("upload_rom: import failed: {:#}", e);
                sse_send(
                    &sse_tx,
                    "import_rom_error",
                    json!({
                        "name": label,
                        "success": false,
                        "error": format!("{:#}", e),
                        "message": format!("Failed to import \"{}\": {:#}", label, e),
                    }),
                );
            }
        }
    });

    Response::builder()
        .status(StatusCode::ACCEPTED)
        .body(Body::from("Import queued"))
        .unwrap()
}

/// Import a patch file for a specific ROM, uploaded by the client.
///
/// Takes multipart with a `file` field and a `rom` field (the id of the target
/// ROM). Returns as soon as the work is queued; progress arrives over SSE.
async fn upload_patch(State(state): State<AppState>, mut multipart: Multipart) -> Response<Body> {
    let mut upload = None;
    let mut rom_id = None;

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(e) => {
                log::error!("upload_patch: multipart error: {}", e);
                return bad_request(e.to_string());
            }
        };

        let field_name = field.name().map(|name| name.to_owned());
        let file_name = field.file_name().map(|name| name.to_owned());

        match field_name.as_deref() {
            Some("file") => {
                // Straight to disk, a chunk at a time, for the same reason as
                // `upload_rom` — although patches are small, the path is shared.
                let filename = safe_filename(file_name.as_deref().unwrap_or_default());
                match stream_field_to_directory(field, &filename).await {
                    Ok(directory) => upload = Some((filename, directory)),
                    Err(e) => {
                        log::error!("upload_patch: failed to store the upload: {:#}", e);
                        return bad_request(e.to_string());
                    }
                }
            }
            Some("rom") => match field.text().await {
                Ok(text) if !text.trim().is_empty() => match text.trim().parse::<i64>() {
                    Ok(id) => rom_id = Some(id),
                    Err(_) => log::warn!("upload_patch: invalid rom field: {text}"),
                },
                Ok(_) => {}
                Err(e) => log::warn!("upload_patch: failed to read rom field: {}", e),
            },
            other => {
                log::debug!("upload_patch: skipping unknown field {:?}", other);
                let _ = field.bytes().await;
            }
        }
    }

    let (filename, directory) = match upload {
        Some(upload) => upload,
        None => return bad_request("No file provided".to_string()),
    };
    let rom_id = match rom_id {
        Some(rom_id) => rom_id,
        None => {
            drop(directory);
            return bad_request("No rom provided".to_string());
        }
    };

    // Reject an unknown ROM up front so the client gets a 400 rather than a task
    // that only dies once it tries to resolve the id.
    {
        let pool = state.pool.clone();
        let mut connection = pool.acquire().await.unwrap();
        if find_rom_by_id_opt(&mut connection, rom_id).await.is_none() {
            drop(directory);
            return bad_request(format!("ROM {rom_id} not found"));
        }
    }

    let sse_tx = state.sse_tx.clone();
    let pool = state.pool.clone();
    let patch_path = directory.path().join(&filename);

    tokio::spawn(async move {
        let mut connection = pool.acquire().await.unwrap();
        let progress_bar = ProgressBar::hidden();

        sse_send(
            &sse_tx,
            "import_patch_started",
            json!({ "name": filename, "message": format!("Importing patch \"{}\"", filename) }),
        );

        let outcome = async {
            let patch_format = parse_patch(&patch_path).await?
                .ok_or_else(|| anyhow::anyhow!("Unsupported patch format"))?;
            import_patch(
                &mut connection,
                &progress_bar,
                &patch_path,
                &patch_format,
                false,
                false,
                Some(rom_id),
            )
            .await?;
            Ok::<(), anyhow::Error>(())
        }
        .await;

        match outcome {
            Ok(()) => {
                sse_send(
                    &sse_tx,
                    "import_patch_complete",
                    json!({ "name": filename, "success": true, "message": format!("Imported patch \"{}\"", filename) }),
                );
            }
            Err(e) => {
                log::error!("upload_patch: import failed: {:#}", e);
                sse_send(
                    &sse_tx,
                    "import_patch_error",
                    json!({ "name": filename, "success": false, "error": format!("{:#}", e), "message": format!("Failed to import patch \"{}\": {:#}", filename, e) }),
                );
            }
        }
        // `directory` is dropped here, removing the now-imported temporary file.
    });

    Response::builder()
        .status(StatusCode::ACCEPTED)
        .body(Body::from("Import queued"))
        .unwrap()
}

fn bad_request(message: String) -> Response<Body> {
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(Body::from(message))
        .unwrap()
}

/// Import an uploaded PlayStation 3 IRD against a given system.
async fn upload_ird(State(state): State<AppState>, mut multipart: Multipart) -> Response<Body> {
    let mut upload = None;
    let mut system_id = None;

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(e) => {
                log::error!("upload_ird: multipart error: {}", e);
                return bad_request(e.to_string());
            }
        };

        let field_name = field.name().map(|name| name.to_owned());
        let file_name = field.file_name().map(|name| name.to_owned());

        match field_name.as_deref() {
            Some("file") => {
                let filename = safe_filename(file_name.as_deref().unwrap_or_default());
                match stream_field_to_directory(field, &filename).await {
                    Ok(directory) => upload = Some((filename, directory)),
                    Err(e) => {
                        log::error!("upload_ird: failed to store the upload: {:#}", e);
                        return bad_request(e.to_string());
                    }
                }
            }
            Some("system") => match field.text().await {
                Ok(text) if !text.trim().is_empty() => match text.trim().parse::<i64>() {
                    Ok(id) => system_id = Some(id),
                    Err(_) => log::warn!("upload_ird: invalid system field: {text}"),
                },
                Ok(_) => {}
                Err(e) => log::warn!("upload_ird: failed to read system field: {}", e),
            },
            other => {
                log::debug!("upload_ird: skipping unknown field {:?}", other);
                let _ = field.bytes().await;
            }
        }
    }

    let (filename, directory) = match upload {
        Some(upload) => upload,
        None => return bad_request("No file provided".to_string()),
    };
    let system_id = match system_id {
        Some(id) => id,
        None => {
            drop(directory);
            return bad_request("No system provided".to_string());
        }
    };

    // Reject an unknown system up front so the client gets a 400, and keep the
    // name the unattended import matches against.
    let system_name = {
        let pool = state.pool.clone();
        let mut connection = pool.acquire().await.unwrap();
        match find_system_by_id_opt(&mut connection, system_id).await {
            Some(system) => system.name,
            None => {
                drop(directory);
                return bad_request(format!("System {system_id} not found"));
            }
        }
    };

    let sse_tx = state.sse_tx.clone();
    let pool = state.pool.clone();

    tokio::spawn(async move {
        let mut connection = pool.acquire().await.unwrap();
        let progress_bar = ProgressBar::hidden();
        let ird_path = directory.path().join(&filename);

        sse_send(
            &sse_tx,
            "import_irds_started",
            json!({ "name": filename, "message": format!("Importing IRD \"{}\"", filename) }),
        );

        // The IRD path is the single positional; `--system` keeps it unattended.
        let arguments = vec![
            "import-irds".to_string(),
            ird_path.to_string_lossy().to_string(),
            "--system".to_string(),
            system_name,
        ];
        let matches = import_irds::subcommand().get_matches_from(arguments);

        match import_irds::main(&mut connection, &matches, &progress_bar).await {
            Ok(()) => {
                sse_send(
                    &sse_tx,
                    "import_irds_complete",
                    json!({ "name": filename, "success": true, "message": format!("Imported IRD \"{}\"", filename) }),
                );
            }
            Err(e) => {
                log::error!("upload_ird: import failed: {:#}", e);
                sse_send(
                    &sse_tx,
                    "import_irds_error",
                    json!({ "name": filename, "success": false, "error": format!("{:#}", e), "message": format!("Failed to import IRD \"{}\": {:#}", filename, e) }),
                );
            }
        }
        // `directory` is dropped here, removing the now-imported temporary file.
    });

    Response::builder()
        .status(StatusCode::ACCEPTED)
        .body(Body::from("Import queued"))
        .unwrap()
}

/// Where a ROM to import came from.
enum RomSource {
    /// Already written to a temporary directory, which is carried along so it
    /// outlives the import rather than being cleaned up under it.
    Upload {
        filename: String,
        directory: tempfile::TempDir,
    },
    Url(String),
}

impl RomSource {
    /// What to call it in a notification.
    fn label(&self) -> String {
        match self {
            RomSource::Upload { filename, .. } => filename.clone(),
            RomSource::Url(url) => url_filename(url),
        }
    }
}

/// Write a multipart field to a fresh temporary directory under the given name.
async fn stream_field_to_directory(
    mut field: axum::extract::multipart::Field<'_>,
    filename: &str,
) -> Result<tempfile::TempDir> {
    let directory = tempfile::TempDir::new()?;
    let mut file = tokio::fs::File::create(directory.path().join(filename)).await?;
    while let Some(chunk) = field.chunk().await? {
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    Ok(directory)
}

/// The last path component of a name chosen by the client, with anything that
/// could climb out of the directory it gets joined onto removed.
fn safe_filename(name: &str) -> String {
    let name = name
        .rsplit(['/', '\\'])
        .find(|segment| !segment.is_empty() && *segment != "." && *segment != "..")
        .unwrap_or_default();
    if name.is_empty() {
        "download".to_string()
    } else {
        name.to_string()
    }
}

/// The last path segment of a URL, which is the closest thing it offers to a
/// file name. Falls back to something recognisable when there is none.
///
/// The segment is percent-decoded, so a link ending in `Game%20(USA).zip` lands
/// as `Game (USA).zip` rather than keeping the escapes in its name. Decoding can
/// itself introduce separators (`%2F`, or a `..`), and the result is joined onto
/// a directory, so anything that would climb out of it is dropped afterwards.
fn url_filename(url: &str) -> String {
    let without_query = url.split(['?', '#']).next().unwrap_or(url);
    // Drop the scheme and authority first, or a URL with no path at all ends up
    // named after the host.
    let path = without_query
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(without_query)
        .split_once('/')
        .map(|(_, path)| path)
        .unwrap_or_default()
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or_default();
    let decoded = percent_encoding::percent_decode_str(path).decode_utf8_lossy();
    safe_filename(&decoded)
}

/// ROMs can be large, but not this large: the cap exists to stop a hostile or
/// misconfigured URL from filling the disk.
const MAX_ROM_DOWNLOAD_SIZE: u64 = 100 * 1024 * 1024 * 1024;

/// Land the ROM in a temporary directory and hand it to `import-roms`.
///
/// Unattended, because there is no one at a terminal to answer a prompt: a file
/// matching several games auto-selects by closest name rather than asking,
/// unless the caller chose to skip such files.
async fn import_rom_source(
    connection: &mut sqlx::SqliteConnection,
    progress_bar: &ProgressBar,
    source: RomSource,
    system: Option<&str>,
    unattended: Option<&str>,
) -> Result<()> {
    // "first" when absent: it is the mode that resolves an ambiguous match on
    // its own, and this function is only ever called from the server, where no
    // one is at a terminal.
    let unattended = match unattended {
        Some(mode) if mode == "skip" || mode == "first" => mode,
        Some(mode) => bail!(
            "Invalid unattended mode '{}'; expected 'skip' or 'first'",
            mode
        ),
        None => "first",
    };
    // Held for the rest of the function: dropping it deletes the file being
    // imported.
    let (path, _tmp_directory) = match source {
        RomSource::Upload {
            filename,
            directory,
        } => {
            let path = directory.path().join(&filename);
            (path, directory)
        }
        RomSource::Url(url) => {
            // The server fetches this on the user's behalf, so it must not be
            // used to reach past the web: http(s) only, a connect timeout so a
            // dead host cannot hang the import, and a size cap so a hostile
            // or misconfigured host cannot fill the disk.
            if !url.starts_with("http://") && !url.starts_with("https://") {
                bail!("Only http:// and https:// URLs can be imported");
            }
            let directory = tempfile::TempDir::new()?;
            let path = directory.path().join(url_filename(&url));
            let client = reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(30))
                .build()?;
            // Streamed for the same reason uploads are: the response is a ROM.
            let mut response = client.get(&url).send().await?.error_for_status()?;
            let mut file = tokio::fs::File::create(&path).await?;
            let mut written = 0u64;
            while let Some(chunk) = response.chunk().await? {
                written += chunk.len() as u64;
                if written > MAX_ROM_DOWNLOAD_SIZE {
                    bail!(
                        "Download exceeds the {} GiB limit",
                        MAX_ROM_DOWNLOAD_SIZE / 1024 / 1024 / 1024
                    );
                }
                file.write_all(&chunk).await?;
            }
            file.flush().await?;
            (path, directory)
        }
    };

    let path = path.as_os_str().to_str().unwrap().to_string();
    let mut arguments = vec![
        "import-roms".to_string(),
        "-u".to_string(),
        unattended.to_string(),
    ];
    if let Some(system) = system {
        arguments.push("-s".to_string());
        arguments.push(system.to_string());
    }
    arguments.push(path);

    let matches = super::import_roms::subcommand().try_get_matches_from(arguments)?;
    super::import_roms::main(connection, &matches, progress_bar).await
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

        sse_send(
            &sse_tx,
            "import_dat_started",
            json!({
                "filename": filename,
                "message": format!("Importing '{}'", filename),
            }),
        );

        let tmp_dir = match tempfile::TempDir::new() {
            Ok(d) => d,
            Err(e) => {
                sse_send(
                    &sse_tx,
                    "import_dat_error",
                    json!({
                        "filename": filename,
                        "error": e.to_string(),
                        "message": format!("Failed to create temp directory: {}", e),
                    }),
                );
                return;
            }
        };

        let upload_path = tmp_dir.path().join(&filename);
        if let Err(e) = tokio::fs::write(&upload_path, &data).await {
            sse_send(
                &sse_tx,
                "import_dat_error",
                json!({
                    "filename": filename,
                    "error": e.to_string(),
                    "message": format!("Failed to save uploaded file: {}", e),
                }),
            );
            return;
        }

        let results =
            process_dat_upload(&mut connection, &progress_bar, &upload_path, update).await;

        for result in results {
            match result {
                Ok(ImportDatResult::Imported(summary)) => {
                    sse_send(
                        &sse_tx,
                        "import_dat_complete",
                        json!({
                            "system_name": summary.system_name,
                            "system_version": summary.system_version,
                            "game_count": summary.game_count,
                            "message": format!("Imported '{}' ({} games)", summary.system_name, summary.game_count),
                        }),
                    );
                }
                Ok(ImportDatResult::UpToDate(system_name)) => {
                    sse_send(
                        &sse_tx,
                        "import_dat_complete",
                        json!({
                            "skipped": true,
                            "message": format!("'{}' is already up to date", system_name),
                        }),
                    );
                }
                Ok(ImportDatResult::Skipped) => {}
                Err(e) => {
                    sse_send(
                        &sse_tx,
                        "import_dat_error",
                        json!({
                            "filename": filename,
                            "error": e.to_string(),
                            "message": format!("Failed to import '{}': {}", filename, e),
                        }),
                    );
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
