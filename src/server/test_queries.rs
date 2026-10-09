use super::super::config::{MUTEX, set_rom_directory, set_tmp_directory};
use super::super::import_dats;
use super::*;
use async_graphql::Result;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};
use tokio::select;
use tokio::time::{Duration, sleep};

async fn gql(client: &reqwest::Client, body: &str) -> Value {
    let string = client
        .post("http://127.0.0.1:8011/graphql")
        .body(body.to_string())
        .header("Content-Type", "application/json")
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    serde_json::from_str(&string).unwrap()
}

#[tokio::test]
async fn test() -> Result<()> {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let progress_bar = indicatif::ProgressBar::hidden();
    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // when
    let matches = subcommand().get_matches_from(["server", "--port", "8011"]);
    let server = async move {
        main(pool, &matches).await.unwrap();
    };

    let client = async move {
        sleep(Duration::from_millis(100)).await;
        let client = reqwest::Client::new();

        // version
        let v = gql(&client, r#"{"query":"{ version }"}"#).await;
        assert_eq!(v["data"]["version"], json!(env!("CARGO_PKG_VERSION")));

        // dependencies: every listed tool, version null when absent
        let v = gql(&client, r#"{"query":"{ dependencies { name version } }"}"#).await;
        let deps = v["data"]["dependencies"].as_array().unwrap();
        assert!(deps.iter().any(|d| d["name"] == json!("chdman")));
        assert!(deps.iter().any(|d| d["name"] == json!("sevenz-rust2")));
        // sorted by name
        let names: Vec<&str> = deps.iter().map(|d| d["name"].as_str().unwrap()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);

        // downloadable_systems returns a list (Redump catalog)
        let v = gql(&client, r#"{"query":"{ downloadableSystems }"}"#).await;
        assert!(v["data"]["downloadableSystems"].is_array());

        // system_settings merges global settings for the system
        let v = gql(
            &client,
            r#"{"query":"{ systemSettings(systemId: 1) { key value } }"}"#,
        )
        .await;
        assert!(v["data"]["systemSettings"].is_array());

        // game_information parses a No-Intro style name
        let v = gql(
            &client,
            &json!({
                "query": "{ gameInformation(gameName: \"Test Game (USA, Europe) (En,Fr) (Beta)\") { title regions languages release flags } }"
            })
            .to_string(),
        )
        .await;
        let info = &v["data"]["gameInformation"];
        assert_eq!(info["title"], json!("Test Game"));
        assert_eq!(info["release"], json!("Beta"));
        assert!(!info["regions"].as_array().unwrap().is_empty());
        assert!(!info["languages"].as_array().unwrap().is_empty());

        // ignored: a non-arcade system's roms are never ignored
        let v = gql(
            &client,
            r#"{"query":"{ roms(gameId: 1) { name ignored(systemId: 1) } }"}"#,
        )
        .await;
        let roms = v["data"]["roms"].as_array().unwrap();
        assert!(!roms.is_empty());
        assert!(roms.iter().all(|r| r["ignored"] == json!(false)));
    };

    select! {
        _ = server => {}
        _ = client => {}
    }

    Ok(())
}
