use super::database::*;
use super::mutation::Mutation;
use super::query::{GameLoader, QueryRoot, RomfileLoader, SystemLoader};
use async_graphql::dataloader::DataLoader;
use async_graphql::{EmptySubscription, Schema};
use async_graphql_axum::GraphQL;
use axum::{
    body::Body,
    extract::Path,
    http::{header, Response, StatusCode},
    routing::{get, post_service},
    serve, Router,
};
use clap::{Arg, ArgMatches, Command};
use http_types::mime::{BYTE_STREAM, HTML};
use http_types::Mime;
use lazy_static::lazy_static;
use once_cell::sync::OnceCell;
use rust_embed::RustEmbed;
use simple_error::SimpleResult;
use sqlx::sqlite::SqlitePool;
use tokio::net::TcpListener;
use tokio::select;
use tokio::signal::ctrl_c;

lazy_static! {
    pub static ref POOL: OnceCell<SqlitePool> = OnceCell::new();
}

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
            .body(Body::from(Vec::new()))
            .unwrap(),
    }
}

async fn run(address: &str, port: &str) -> SimpleResult<()> {
    let schema = Schema::build(QueryRoot, Mutation, EmptySubscription)
        .data(DataLoader::new(SystemLoader, tokio::task::spawn))
        .data(DataLoader::new(GameLoader, tokio::task::spawn))
        .data(DataLoader::new(RomfileLoader, tokio::task::spawn))
        .finish();

    let app = Router::new()
        .route("/graphql", post_service(GraphQL::new(schema)))
        .route("/*path", get(serve_asset))
        .route("/", get(serve_index));

    let listener = TcpListener::bind(format!("{}:{}", address, port))
        .await
        .unwrap();
    serve(listener, app.into_make_service()).await.unwrap();

    Ok(())
}

pub async fn main(pool: SqlitePool, matches: &ArgMatches) -> SimpleResult<()> {
    POOL.set(pool).expect("Failed to set database pool");

    select! {
        Ok(()) = ctrl_c() => {
            close_connection(POOL.get().unwrap()).await;
        }
        Ok(()) = run(matches.get_one::<String>("ADDRESS").unwrap(), matches.get_one::<String>("PORT").unwrap()) => {}
    }
    Ok(())
}

#[cfg(test)]
mod test_server;
