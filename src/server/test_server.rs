extern crate serde_json;

use super::super::config::{set_rom_directory, set_tmp_directory, MUTEX};
use super::super::database::*;
use super::super::import_dats;
use super::super::import_roms;
use super::super::sort_roms;
use super::super::util::*;
use super::*;
use async_graphql::Result;
use async_std::fs;
use async_std::path::PathBuf;
use async_std::task;
use indicatif::ProgressBar;
use serde_json::{json, Value};
use std::time::Duration;
use tempfile::{NamedTempFile, TempDir};
use tide::Body;

#[async_std::test]
async fn test() -> Result<()> {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    set_rom_directory(PathBuf::from(rom_directory.path()));
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

    let matches = import_dats::subcommand()
        .get_matches_from(&["import-dats", "tests/Test System (20200721).dat"]);
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
