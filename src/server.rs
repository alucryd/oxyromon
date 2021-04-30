extern crate async_ctrlc;
extern crate async_graphql;
extern crate async_graphql_tide;
extern crate async_trait;
extern crate http_types;
extern crate rust_embed;
extern crate tide;

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
use clap::{App, Arg, ArgMatches, SubCommand};
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
#[folder = "public/"]
struct Assets;

pub fn subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name("server")
        .about("Launches the backend server")
        .arg(
            Arg::with_name("ADDRESS")
                .short("a")
                .long("address")
                .help("Specifies the server address")
                .required(false)
                .takes_value(true)
                .default_value("127.0.0.1"),
        )
        .arg(
            Arg::with_name("PORT")
                .short("p")
                .long("port")
                .help("Specifies the server port")
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
        Ok(find_roms_by_game_id(&mut POOL.get().unwrap().acquire().await.unwrap(), game_id).await)
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

#[derive(Clone)]
struct AppState {
    schema: Schema<QueryRoot, EmptyMutation, EmptySubscription>,
}

async fn serve_asset(req: tide::Request<()>) -> tide::Result {
    let file_path = match req.param("path") {
        Ok(path) => path,
        Err(_) => "index.html",
    };
    match Assets::get(file_path) {
        Some(bytes) => {
            let mime = Mime::sniff(bytes.as_ref())
                .or_else(|err| {
                    Mime::from_extension(
                        Path::new(file_path).extension().unwrap().to_str().unwrap(),
                    )
                    .ok_or(err)
                })
                .unwrap_or(BYTE_STREAM);
            Ok(tide::Response::builder(StatusCode::Ok)
                .body(tide::Body::from_bytes(bytes.to_vec()))
                .content_type(mime)
                .build())
        }
        None => Ok(tide::Response::new(StatusCode::NotFound)),
    }
}

pub async fn main(pool: SqlitePool, matches: &ArgMatches<'_>) -> SimpleResult<()> {
    POOL.set(pool).expect("Failed to set database pool");

    let schema = Schema::build(QueryRoot, EmptyMutation, EmptySubscription)
        .data(DataLoader::new(SystemLoader))
        .data(DataLoader::new(GameLoader))
        .data(DataLoader::new(RomfileLoader))
        .finish();

    let ctrlc = CtrlC::new().expect("Cannot use CTRL-C handler");
    ctrlc
        .race(async {
            let mut app = tide::new();

            app.at("/").get(serve_asset);
            app.at("/*path").get(serve_asset);

            app.at("/graphql")
                .post(async_graphql_tide::endpoint(schema));

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
mod tests {
    extern crate serde_json;

    use super::super::config::{set_rom_directory, set_tmp_directory, MUTEX};
    use super::super::database::*;
    use super::super::import_dats;
    use super::super::import_roms;
    use super::super::sort_roms;
    use super::super::util::*;
    use super::*;
    use async_std::fs;
    use async_std::path::PathBuf;
    use async_std::task;
    use indicatif::ProgressBar;
    use serde_json::{json, Value};
    use std::time::Duration;
    use tempfile::{NamedTempFile, TempDir};
    use tide::Body;

    #[async_std::test]
    async fn test_server() -> Result<()> {
        // given
        let _guard = MUTEX.lock().await;

        let test_directory = Path::new("test");
        let progress_bar = ProgressBar::hidden();

        let db_file = NamedTempFile::new().unwrap();
        let pool = establish_connection(db_file.path().to_str().unwrap()).await;
        let mut connection = pool.acquire().await.unwrap();

        let rom_directory = TempDir::new_in(&test_directory).unwrap();
        set_rom_directory(PathBuf::from(rom_directory.path()));
        let tmp_directory = TempDir::new_in(&test_directory).unwrap();
        let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

        let matches = import_dats::subcommand()
            .get_matches_from(&["import-dats", "test/Test System (20200721).dat"]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let romfile_names = vec!["Test Game (USA, Europe).rom", "Test Game (Japan).rom"];
        let mut romfile_paths = vec![];
        for romfile_name in romfile_names {
            let romfile_path = tmp_directory.join(romfile_name);
            fs::copy(test_directory.join(romfile_name), &romfile_path)
                .await
                .unwrap();
            romfile_paths.push(romfile_path);
        }

        let matches = import_roms::subcommand().get_matches_from(&[
            "import-roms",
            romfile_paths.get(0).unwrap().as_os_str().to_str().unwrap(),
            romfile_paths.get(1).unwrap().as_os_str().to_str().unwrap(),
        ]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let matches = sort_roms::subcommand().get_matches_from(&[
            "sort-roms",
            "-a",
            "-y",
            "-g",
            "US",
            "-r",
            "JP",
        ]);
        sort_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();

        let system = find_systems(&mut connection).await.remove(0);

        // when
        let matches = subcommand().get_matches_from(&["server"]);
        task::block_on(async {
            let server: task::JoinHandle<Result<()>> = task::spawn(async move {
                main(pool, &matches).await?;
                Ok(())
            });

            let client: task::JoinHandle<Result<()>> = task::spawn(async move {
                task::sleep(Duration::from_millis(1000)).await;

                let string = surf::post("http://127.0.0.1:8000/graphql")
                    .body(Body::from(
                        r#"{"query":"{ systems { id, name, header { id, name } } }"}"#,
                    ))
                    .header("Content-Type", "application/json")
                    .recv_string()
                    .await?;

                let v: Value = serde_json::from_str(&string)?;
                assert_eq!(
                    v["data"]["systems"],
                    json!(
                        [
                            {
                                "id": 1,
                                "name": "Test System",
                                "header": null
                            }
                        ]
                    )
                );

                let string = surf::post("http://127.0.0.1:8000/graphql")
                    .body(Body::from(
                        r#"{"query":"{ games(systemId: 1) { id, name } }"}"#,
                    ))
                    .header("Content-Type", "application/json")
                    .recv_string()
                    .await?;

                let v: Value = serde_json::from_str(&string)?;
                assert_eq!(
                    v["data"]["games"],
                    json!(
                        [
                            {
                                "id": 5,
                                "name": "Test Game (Asia)"
                            },
                            {
                                "id": 4,
                                "name": "Test Game (Japan)"
                            },
                            {
                                "id": 1,
                                "name": "Test Game (USA, Europe)"
                            },
                            {
                                "id": 6,
                                "name": "Test Game (USA, Europe) (Beta)"
                            },
                            {
                                "id": 3,
                                "name": "Test Game (USA, Europe) (CUE BIN)"
                            },
                            {
                                "id": 2,
                                "name": "Test Game (USA, Europe) (ISO)"
                            }
                        ]
                    )
                );

                let string = surf::post("http://127.0.0.1:8000/graphql")
                    .body(Body::from(
                        r#"{"query":"{ roms(gameId: 1) { id, name, romfile { id, path, size }, game { id, name, system { id, name } } } }"}"#,
                    ))
                    .header("Content-Type", "application/json")
                    .recv_string()
                    .await?;

                let v: Value = serde_json::from_str(&string)?;
                assert_eq!(
                    v["data"]["roms"],
                    json!(
                        [
                            {
                                "id": 1,
                                "name": "Test Game (USA, Europe).rom",
                                "romfile": {
                                    "id": 1,
                                    "path": format!("{}/Test Game (USA, Europe).rom", get_one_region_directory(&mut connection, &progress_bar, &system).await.unwrap().as_os_str().to_str().unwrap()),
                                    "size": 256
                                },
                                "game": {
                                    "id": 1,
                                    "name": "Test Game (USA, Europe)",
                                    "system": {
                                        "id": 1,
                                        "name": "Test System"
                                    }
                                },
                            }
                        ]
                    )
                );

                let string = surf::post("http://127.0.0.1:8000/graphql")
                    .body(Body::from(
                        r#"{"query":"{ totalOriginalSize(systemId: 1), oneRegionOriginalSize(systemId: 1), totalActualSize(systemId: 1), oneRegionActualSize(systemId: 1) }"}"#,
                    ))
                    .header("Content-Type", "application/json")
                    .recv_string()
                    .await?;

                let v: Value = serde_json::from_str(&string)?;
                assert_eq!(
                    v["data"],
                    json!(
                        {
                            "totalOriginalSize": 512,
                            "oneRegionOriginalSize": 256,
                            "totalActualSize": 512,
                            "oneRegionActualSize": 256,
                        }
                    )
                );

                Ok(())
            });

            server.race(client).await?;

            Ok(())
        })
    }
}
