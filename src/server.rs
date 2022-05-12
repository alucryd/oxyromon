use super::database::*;
use super::model::*;
use async_ctrlc::CtrlC;
use async_graphql::dataloader::{DataLoader, Loader};
use async_graphql::{
    ComplexObject, Context, EmptyMutation, EmptySubscription, Error, Object, Result, Schema,
};
use async_std::path::Path;
use async_std::prelude::FutureExt;
use async_trait::async_trait;
use clap::{Arg, ArgMatches, Command};
use futures::stream::TryStreamExt;
use http_types::mime::BYTE_STREAM;
use http_types::{Mime, StatusCode};
use itertools::Itertools;
use lazy_static::lazy_static;
use once_cell::sync::OnceCell;
use rust_embed::RustEmbed;
use simple_error::SimpleResult;
use sqlx::sqlite::SqlitePool;
use std::collections::HashMap;

lazy_static! {
    static ref POOL: OnceCell<SqlitePool> = OnceCell::new();
}

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/target/assets"]
struct Assets;

pub fn subcommand<'a>() -> Command<'a> {
    Command::new("server")
        .about("Launch the backend server")
        .arg(
            Arg::new("ADDRESS")
                .short('a')
                .long("address")
                .help("Specify the server address")
                .required(false)
                .takes_value(true)
                .default_value("127.0.0.1"),
        )
        .arg(
            Arg::new("PORT")
                .short('p')
                .long("port")
                .help("Specify the server port")
                .required(false)
                .takes_value(true)
                .default_value("8000"),
        )
}

#[ComplexObject]
impl System {
    async fn header(&self) -> Result<Option<Header>> {
        Ok(
            find_header_by_system_id(&mut POOL.get().unwrap().acquire().await.unwrap(), self.id)
                .await,
        )
    }
}

#[ComplexObject]
impl Game {
    async fn system(&self, ctx: &Context<'_>) -> Result<Option<System>> {
        Ok(ctx
            .data_unchecked::<DataLoader<SystemLoader>>()
            .load_one(self.system_id)
            .await?)
    }
}

#[ComplexObject]
impl Rom {
    async fn game(&self, ctx: &Context<'_>) -> Result<Option<Game>> {
        Ok(ctx
            .data_unchecked::<DataLoader<GameLoader>>()
            .load_one(self.game_id)
            .await?)
    }

    async fn romfile(&self, ctx: &Context<'_>) -> Result<Option<Romfile>> {
        Ok(match self.romfile_id {
            Some(romfile_id) => {
                ctx.data_unchecked::<DataLoader<RomfileLoader>>()
                    .load_one(romfile_id)
                    .await?
            }
            None => None,
        })
    }
}

pub struct SystemLoader;

#[async_trait]
impl Loader<i64> for SystemLoader {
    type Value = System;
    type Error = Error;

    async fn load(&self, ids: &[i64]) -> Result<HashMap<i64, Self::Value>, Self::Error> {
        let query = format!(
            "
        SELECT *
        FROM systems
        WHERE id in ({})
        ",
            ids.iter().join(",")
        );
        Ok(sqlx::query_as(&query)
            .fetch(&mut POOL.get().unwrap().acquire().await.unwrap())
            .map_ok(|system: System| (system.id, system))
            .try_collect()
            .await?)
    }
}

pub struct GameLoader;

#[async_trait]
impl Loader<i64> for GameLoader {
    type Value = Game;
    type Error = Error;

    async fn load(&self, ids: &[i64]) -> Result<HashMap<i64, Self::Value>, Self::Error> {
        let query = format!(
            "
        SELECT *
        FROM games
        WHERE id in ({})
        ",
            ids.iter().join(",")
        );
        Ok(sqlx::query_as(&query)
            .fetch(&mut POOL.get().unwrap().acquire().await.unwrap())
            .map_ok(|game: Game| (game.id, game))
            .try_collect()
            .await?)
    }
}

pub struct RomfileLoader;

#[async_trait]
impl Loader<i64> for RomfileLoader {
    type Value = Romfile;
    type Error = Error;

    async fn load(&self, ids: &[i64]) -> Result<HashMap<i64, Self::Value>, Self::Error> {
        let query = format!(
            "
        SELECT *
        FROM romfiles
        WHERE id in ({})
        ",
            ids.iter().join(",")
        );
        Ok(sqlx::query_as(&query)
            .fetch(&mut POOL.get().unwrap().acquire().await.unwrap())
            .map_ok(|romfile: Romfile| (romfile.id, romfile))
            .try_collect()
            .await?)
    }
}

struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn systems(&self) -> Result<Vec<System>> {
        Ok(find_systems(&mut POOL.get().unwrap().acquire().await.unwrap()).await)
    }

    async fn games(&self, system_id: i64) -> Result<Vec<Game>> {
        Ok(
            find_games_by_system_id(&mut POOL.get().unwrap().acquire().await.unwrap(), system_id)
                .await,
        )
    }

    async fn roms(&self, game_id: i64) -> Result<Vec<Rom>> {
        Ok(
            find_roms_by_game_id_parents(
                &mut POOL.get().unwrap().acquire().await.unwrap(),
                game_id,
            )
            .await,
        )
    }

    async fn total_original_size(&self, system_id: i64) -> Result<i64> {
        let sql = format!(
            "
                SELECT COALESCE(SUM(r.size), 0)
                FROM roms r
                JOIN games g ON r.game_id = g.id
                WHERE r.romfile_id IS NOT NULL
                AND g.system_id = {};
            ",
            system_id
        );
        let row: (i64,) = sqlx::query_as(&sql)
            .fetch_one(&mut POOL.get().unwrap().acquire().await.unwrap())
            .await?;
        Ok(row.0)
    }

    async fn one_region_original_size(&self, system_id: i64) -> Result<i64> {
        let sql = format!(
            "
                SELECT COALESCE(SUM(r.size), 0)
                FROM roms r
                JOIN games g ON r.game_id = g.id
                WHERE r.romfile_id IS NOT NULL
                AND g.sorting = 1
                AND g.system_id = {};
            ",
            system_id
        );
        let row: (i64,) = sqlx::query_as(&sql)
            .fetch_one(&mut POOL.get().unwrap().acquire().await.unwrap())
            .await?;
        Ok(row.0)
    }

    async fn total_actual_size(&self, system_id: i64) -> Result<i64> {
        let sql = format!(
            "
                SELECT COALESCE(SUM(rf.size), 0)
                FROM romfiles rf
                WHERE rf.id IN (
                    SELECT DISTINCT(r.romfile_id) FROM roms r
                    JOIN games g ON r.game_id = g.id
                    WHERE r.romfile_id IS NOT NULL
                    AND g.system_id = {}
                );
            ",
            system_id
        );
        let row: (i64,) = sqlx::query_as(&sql)
            .fetch_one(&mut POOL.get().unwrap().acquire().await.unwrap())
            .await?;
        Ok(row.0)
    }

    async fn one_region_actual_size(&self, system_id: i64) -> Result<i64> {
        let sql = format!(
            "
                SELECT COALESCE(SUM(rf.size), 0)
                FROM romfiles rf
                WHERE rf.id IN (
                    SELECT DISTINCT(r.romfile_id) FROM roms r
                    JOIN games g ON r.game_id = g.id
                    WHERE r.romfile_id IS NOT NULL
                    AND g.sorting = 1
                    AND g.system_id = {}
                );
            ",
            system_id
        );
        let row: (i64,) = sqlx::query_as(&sql)
            .fetch_one(&mut POOL.get().unwrap().acquire().await.unwrap())
            .await?;
        Ok(row.0)
    }
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

    let schema = Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
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

            let address = matches.value_of("ADDRESS").unwrap();
            let port = matches.value_of("PORT").unwrap();
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
