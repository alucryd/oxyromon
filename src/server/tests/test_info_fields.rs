//! The info fields the web's modals ask lazily: a store told through
//! `systemStore`, a game told through `gameSizes`, one game in one store.

use super::*;
use crate::config::{set_bool, set_string};
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261114) (Game Boy).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe).gb").await;
    let declared = tokio::fs::metadata(&romfile_path).await.unwrap().len();
    env.import_roms(&[&romfile_path]).await;

    // and the game's ORE in the system's store
    let system = env.system().await;
    set_bool(&mut env.connection, "ORE_STORE", true, Some(system.id)).await;
    set_string(
        &mut env.connection,
        "ORE_STORE_BATCH",
        "game",
        Some(system.id),
    )
    .await;
    env.run(&["convert-roms", "-f", "ORE", "-a", "-c"])
        .await
        .unwrap();

    let (sse_tx, _) = broadcast::channel::<SseMessage>(100);
    let schema = build_schema(
        env.pool.clone(),
        sse_tx,
        StoreIndexLoader::new(env.pool.clone()),
    );

    // when the info modal asks the system about its store
    let response = schema
        .execute("{ systemStore(systemId: 1) { path size dedupSaved parityCost systemDedupSaved systemParityCost systems } }")
        .await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);

    // then it names the store beneath the system, weighed, with the
    // system alone in its group: one game shares its chunks with nobody,
    // so its part of the dedup is nil and its part of the parity is all
    // of it
    let store = env.system_directory(&system).await.join(ore_rs::STORE_DIR);
    let stats = ore_rs::store_stats(&store).unwrap();
    let data = response.data.into_json().unwrap();
    let store_info = &data["systemStore"];
    assert_eq!(
        store_info["path"].as_str().unwrap(),
        store.to_str().unwrap()
    );
    assert!(store_info["size"].as_i64().unwrap() > 0);
    assert_eq!(
        store_info["dedupSaved"].as_i64().unwrap(),
        stats.dedup_saved as i64
    );
    assert_eq!(
        store_info["parityCost"].as_i64().unwrap(),
        stats.parity as i64
    );
    assert_eq!(store_info["systemDedupSaved"].as_i64().unwrap(), 0);
    // The one game pays the footers; the store's figure counts the room no
    // game lies in too, so the two agree to within that envelope.
    let paid = store_info["systemParityCost"].as_i64().unwrap();
    assert!(paid > 0, "the game pays its part of the parity");
    assert!(
        (stats.parity as i64 - paid).abs() < stats.parity as i64 / 8,
        "{paid} against the store's {}",
        stats.parity
    );
    assert_eq!(store_info["systems"].as_array().unwrap().len(), 1);

    // and the request the system modal actually sends — this text, verbatim
    // from `frontend/src/api.rs` — validates against the schema and answers
    // in the shape `frontend/src/model.rs::SystemDetails` deserializes: the
    // system's facts nested under `system`, the four sizes as root siblings,
    // and the store under the server's key, `systemStore`. Nothing else in
    // the pipeline checks the modal's query text against the live schema:
    // clippy cannot see the string, and the frontend has no native test.
    let response = schema
        .execute(
            "{ system(systemId: 1) { name version gamesComplete gamesTotal completion } \
             totalOriginalSize(systemId: 1) oneRegionOriginalSize(systemId: 1) \
             totalActualSize(systemId: 1) oneRegionActualSize(systemId: 1) \
             systemStore(systemId: 1) { path size dedupSaved parityCost systemDedupSaved systemParityCost systems } }",
        )
        .await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    let data = response.data.into_json().unwrap();
    assert_eq!(
        data["system"]["name"].as_str().unwrap(),
        "Test System (Game Boy)"
    );
    assert_eq!(data["system"]["version"].as_str().unwrap(), "20261114");
    assert_eq!(data["system"]["gamesComplete"].as_i64().unwrap(), 1);
    assert_eq!(data["system"]["gamesTotal"].as_i64().unwrap(), 1);
    assert!(data["system"]["completion"].is_i64());
    // The four sizes answer as root siblings. Their exact figures are the
    // sizes' own tests' business (test_server.rs); here the point is that
    // the modal's one-request query validates and returns each as a number
    // beside the nested system, not buried inside it. (Post-conversion the
    // original sizes read 0 — the ROM rows shed their romfile links to the
    // old files, and the original-size query counts only linked ROMs.)
    assert!(data["totalOriginalSize"].is_i64());
    assert!(data["oneRegionOriginalSize"].is_i64());
    assert!(data["totalActualSize"].is_i64());
    assert!(data["oneRegionActualSize"].is_i64());
    assert!(data["systemStore"]["path"].as_str().unwrap() == store.to_str().unwrap());

    // and the game modal's other literal request, the one that feeds its
    // ROM and file lists
    let response = schema
        .execute("{ roms(gameId: 1) { id name size crc md5 sha1 romfile { id path size } } }")
        .await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    let roms = response.data.into_json().unwrap()["roms"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(roms.len(), 1);
    assert!(roms[0]["crc"].is_string());
    assert!(roms[0]["romfile"]["path"].is_string());

    // and when the game's own modal asks what the game weighs: the DAT's
    // figure is the file it declared, the disk's weighs the share of one
    // game alone in its store, and the ORE's three figures answer
    let response = schema
        .execute(
            "{ gameSizes(gameId: 1) { originalSize actualSize oreOwn oreShare oreStandalone } }",
        )
        .await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    let data = response.data.into_json().unwrap();
    let sizes = &data["gameSizes"];
    assert_eq!(sizes["originalSize"].as_i64().unwrap(), declared as i64);
    let actual = sizes["actualSize"].as_i64().unwrap();
    assert!(actual > 0, "the game weighs something on disk");
    let own = sizes["oreOwn"].as_i64().unwrap();
    let share = sizes["oreShare"].as_i64().unwrap();
    let standalone = sizes["oreStandalone"].as_i64().unwrap();
    assert!(own > 0 && own <= share, "{own} against {share}");
    assert!(
        (share - actual).abs() <= actual / 8,
        "{share} against the actual {actual}"
    );
    assert!(standalone >= share, "{standalone} against {share}");

    // and a system with no store tells its modal nothing
    let response = schema
        .execute("{ systemStore(systemId: 9999) { path } }")
        .await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);
    assert!(response.data.into_json().unwrap()["systemStore"].is_null());
}
