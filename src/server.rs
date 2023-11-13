use super::database::*;
use super::mutation::Mutation;
use super::query::{GameLoader, QueryRoot, RomfileLoader, SystemLoader};
use async_graphql::dataloader::DataLoader;
use async_graphql::{EmptySubscription, Schema};
use clap::{Arg, ArgMatches, Command};
use http_types::mime::BYTE_STREAM;
use http_types::{Mime, StatusCode};
use lazy_static::lazy_static;
use once_cell::sync::OnceCell;
use rust_embed::RustEmbed;
use simple_error::SimpleResult;
use sqlx::sqlite::SqlitePool;
use std::path::Path;
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

async fn serve_asset(req: tide::Request<()>) -> tide::Result {
    let file_path = req.param("path").unwrap_or("index.html");
    match Assets::get(file_path) {
        Some(file) => {
            let mime = Mime::sniff(file.data.as_ref())
                .or_else(|err| {
                    Mime::from_extension(
                        Path::new(file_path).extension().unwrap().to_str().unwrap(),
                    )
                    .ok_or(err)
                })
                .unwrap_or(BYTE_STREAM);
            Ok(tide::Response::builder(StatusCode::Ok)
                .body(tide::Body::from_bytes(file.data.to_vec()))
                .content_type(mime)
                .build())
        }
        None => Ok(tide::Response::new(StatusCode::NotFound)),
    }
}

async fn run(address: &str, port: &str) -> SimpleResult<()> {
    let schema = Schema::build(QueryRoot, Mutation, EmptySubscription)
        .data(DataLoader::new(SystemLoader, tokio::task::spawn))
        .data(DataLoader::new(GameLoader, tokio::task::spawn))
        .data(DataLoader::new(RomfileLoader, tokio::task::spawn))
        .finish();

    let mut app = tide::new();
    app.at("/").get(serve_asset);
    app.at("/*path").get(serve_asset);
    app.at("/graphql").post(async_graphql_tide::graphql(schema));
    try_with!(
        app.listen(format!("{}:{}", address, port)).await,
        "Failed to run server"
    );
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
