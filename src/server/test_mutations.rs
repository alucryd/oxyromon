use super::super::config::{MUTEX, set_rom_directory, set_tmp_directory};
use super::*;
use async_graphql::Result;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};
use tokio::select;
use tokio::time::{Duration, sleep};

async fn gql(client: &reqwest::Client, body: &str) -> Value {
    let string = client
        .post("http://127.0.0.1:8010/graphql")
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

fn setting_value(settings: &Value, key: &str) -> Option<String> {
    settings
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["key"] == json!(key))
        .and_then(|s| s["value"].as_str())
        .map(|s| s.to_string())
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
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    // when
    let matches = subcommand().get_matches_from(["server", "--port", "8010"]);
    let server = async move {
        main(pool, &matches).await.unwrap();
    };

    let client = async move {
        sleep(Duration::from_millis(100)).await;
        let client = reqwest::Client::new();

        // set_bool round-trips
        let v = gql(
            &client,
            r#"{"query":"mutation { setBool(key: \"PREFER_PARENTS\", value: true) }"}"#,
        )
        .await;
        assert_eq!(v["data"]["setBool"], json!(true));
        let v = gql(&client, r#"{"query":"{ settings { key value } }"}"#).await;
        assert_eq!(
            setting_value(&v["data"]["settings"], "PREFER_PARENTS"),
            Some("true".to_string())
        );

        // add_to_list / remove_from_list round-trip on a free-form list
        gql(
            &client,
            r#"{"query":"mutation { addToList(key: \"REGIONS_ALL\", value: \"us\") }"}"#,
        )
        .await;
        gql(
            &client,
            r#"{"query":"mutation { addToList(key: \"REGIONS_ALL\", value: \"eu\") }"}"#,
        )
        .await;
        let v = gql(&client, r#"{"query":"{ settings { key value } }"}"#).await;
        let regions = setting_value(&v["data"]["settings"], "REGIONS_ALL").unwrap();
        assert!(regions.contains("us"), "regions: {}", regions);
        assert!(regions.contains("eu"), "regions: {}", regions);

        gql(
            &client,
            r#"{"query":"mutation { removeFromList(key: \"REGIONS_ALL\", value: \"us\") }"}"#,
        )
        .await;
        let v = gql(&client, r#"{"query":"{ settings { key value } }"}"#).await;
        let regions = setting_value(&v["data"]["settings"], "REGIONS_ALL").unwrap();
        assert!(!regions.contains("us"), "regions: {}", regions);
        assert!(regions.contains("eu"), "regions: {}", regions);

        // validated string settings accept a known variant
        let v = gql(
            &client,
            r#"{"query":"mutation { setPreferRegions(value: \"broad\") }"}"#,
        )
        .await;
        assert_eq!(v["data"]["setPreferRegions"], json!(true));
        let v = gql(&client, r#"{"query":"{ settings { key value } }"}"#).await;
        assert_eq!(
            setting_value(&v["data"]["settings"], "PREFER_REGIONS"),
            Some("broad".to_string())
        );

        let v = gql(
            &client,
            r#"{"query":"mutation { setPreferVersions(value: \"new\") }"}"#,
        )
        .await;
        assert_eq!(v["data"]["setPreferVersions"], json!(true));

        let v = gql(
            &client,
            r#"{"query":"mutation { setSubfolderScheme(key: \"SUBFOLDER_SCHEME\", value: \"alpha\") }"}"#,
        )
        .await;
        assert_eq!(v["data"]["setSubfolderScheme"], json!(true));

        // set_directory accepts an existing directory
        let dir = tmp_directory.to_str().unwrap().replace('\\', "/");
        let v = gql(
            &client,
            &json!({
                "query": format!(
                    "mutation {{ setDirectory(key: \"TMP_DIRECTORY\", value: \"{}\") }}",
                    dir
                )
            })
            .to_string(),
        )
        .await;
        assert_eq!(v["data"]["setDirectory"], json!(true));

        // a value rejected by the validator surfaces as a GraphQL error, not a panic
        let v = gql(
            &client,
            r#"{"query":"mutation { setPreferRegions(value: \"not-a-region\") }"}"#,
        )
        .await;
        assert!(
            v["errors"].is_array(),
            "expected validator error, got: {}",
            v
        );
        assert!(v["data"].is_null() || v["data"]["setPreferRegions"].is_null());

        // actions refuse what they cannot run before queuing anything
        for (query, error) in [
            (r#"sortRoms(systemId: 999)"#, "System 999 not found"),
            (r#"purgeIrds(systemId: 999)"#, "System 999 not found"),
            (
                r#"convertRoms(systemId: 999, format: \"CHD\")"#,
                "System 999 not found",
            ),
            (
                r#"purgeRoms(missing: false, orphan: false, trash: false, foreign: false)"#,
                "No ROM files selected to purge",
            ),
        ] {
            let v = gql(&client, &format!(r#"{{"query":"mutation {{ {query} }}"}}"#)).await;
            assert_eq!(v["errors"][0]["message"], json!(error), "{query}: {v}");
        }
    };

    select! {
        _ = server => {}
        _ = client => {}
    }

    Ok(())
}
