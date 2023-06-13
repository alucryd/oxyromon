use super::database::*;
use super::mutation::Mutation;
use super::query::{GameLoader, QueryRoot, RomfileLoader, SystemLoader};
use async_ctrlc::CtrlC;
use async_graphql::dataloader::DataLoader;
use async_graphql::{EmptySubscription, Schema};
use async_std::path::Path;
use async_std::prelude::FutureExt;
use clap::{Arg, ArgMatches, Command};
use http_types::mime::BYTE_STREAM;
use http_types::{Mime, StatusCode};
use lazy_static::lazy_static;
use once_cell::sync::OnceCell;
use rust_embed::RustEmbed;
use simple_error::SimpleResult;
use sqlx::sqlite::SqlitePool;

lazy_static! {
    pub static ref POOL: OnceCell<SqlitePool> = OnceCell::new();
}

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/target/assets"]
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

pub async fn main(pool: SqlitePool, matches: &ArgMatches) -> SimpleResult<()> {
    POOL.set(pool).expect("Failed to set database pool");

    let schema = Schema::build(QueryRoot, Mutation, EmptySubscription)
        .data(DataLoader::new(SystemLoader, async_std::task::spawn))
        .data(DataLoader::new(GameLoader, async_std::task::spawn))
        .data(DataLoader::new(RomfileLoader, async_std::task::spawn))
        .finish();

    let ctrlc = CtrlC::new().expect("Cannot use CTRL-C handler");
    ctrlc
        .race(async {
            let mut app = tide::new();

            app.at("/").get(serve_asset);
            app.at("/*path").get(serve_asset);

            app.at("/graphql").post(async_graphql_tide::graphql(schema));

            let address = matches.get_one::<String>("ADDRESS").unwrap();
            let port = matches.get_one::<String>("PORT").unwrap();
            app.listen(format!("{}:{}", address, port))
                .await
                .expect("Failed to run server");
        })
        .await;
    close_connection(POOL.get().unwrap()).await;
    Ok(())
}

#[cfg(test)]
mod test_server;
