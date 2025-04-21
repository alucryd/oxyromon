use super::database::*;
use super::mutation::Mutation;
use super::query::{GameLoader, QueryRoot, RomfileLoader, SystemLoader};
use async_graphql::dataloader::DataLoader;
use async_graphql::{EmptySubscription, Schema};
use async_graphql_axum::GraphQL;
use axum::{
    Router,
    body::Body,
    extract::Path,
    http::{Response, StatusCode, header},
    routing::{get, post_service},
    serve,
};
use clap::{Arg, ArgMatches, Command};
use http_types::Mime;
use http_types::mime::{BYTE_STREAM, HTML};
use rust_embed::RustEmbed;
use simple_error::SimpleResult;
use sqlx::sqlite::SqlitePool;
use tokio::net::TcpListener;
use tokio::{select, signal};

#[derive(RustEmbed)]
#[folder = "target/assets"]
struct Assets;

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

async fn shutdown_signal(pool: SqlitePool) {
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
        _ = ctrl_c => {
            optimize_database(pool).await;
        },
        _ = terminate => {
            optimize_database(pool).await;
        },
    }
}

pub async fn main(pool: SqlitePool, matches: &ArgMatches) -> SimpleResult<()> {
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
        .finish();

    let app = Router::new()
        .route("/graphql", post_service(GraphQL::new(schema)))
        .route("/{*path}", get(serve_asset))
        .route("/", get(serve_index));

    let listener = TcpListener::bind(format!(
        "{}:{}",
        matches.get_one::<String>("ADDRESS").unwrap(),
        matches.get_one::<String>("PORT").unwrap()
    ))
    .await
    .unwrap();

    serve(listener, app.into_make_service())
        .with_graceful_shutdown(shutdown_signal(pool))
        .await
        .unwrap();

    Ok(())
}

#[cfg(test)]
mod test_server;
