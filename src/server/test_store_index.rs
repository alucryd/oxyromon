//! The sizes of a system whose games live in a store, as one GraphQL
//! request asks for them: the store's index is read once, whatever the
//! number of fields that need it.

use super::super::config::{set_bool, set_string};
use super::super::database::find_romfiles;
use super::*;
use crate::test_support::TestEnv;
use sqlx::AssertSqlSafe;
use std::path::Path;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261114) (Game Boy).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe).gb").await;
    env.import_roms(&[&romfile_path]).await;

    env.run(&["sort-roms", "-a", "-y", "-o", "US", "-r", "JP"])
        .await
        .unwrap();

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

    // when one request asks for every size of the system
    let store_index_loader = StoreIndexLoader::new(env.pool.clone());
    let counted = store_index_loader.clone();
    let (sse_tx, _) = broadcast::channel::<SseMessage>(100);
    let schema = build_schema(env.pool.clone(), sse_tx, store_index_loader);
    let response = schema
        .execute("{ totalActualSize(systemId: 1), oneRegionActualSize(systemId: 1), storeDedupSaved(systemId: 1), storeParityCost(systemId: 1) }")
        .await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);

    // then the index was read once, whatever the four fields made of it
    assert_eq!(counted.loads(), 1);

    // and it served the sizes: the preload was used, not a store whose path
    // disagreed with the system's and fell back to the disk
    assert!(
        counted.hits() > 0,
        "the sizes took the preloaded index, not their own read"
    );

    // and the answer is still the store's own
    let data = response.data.into_json().unwrap();
    let system_directory = env.system_directory(&system).await;
    let store = system_directory.join(ore_rs::STORE_DIR);
    let stats = ore_rs::store_stats(&store).unwrap();
    assert_eq!(data["storeDedupSaved"], stats.dedup_saved as i64);
    assert_eq!(data["storeParityCost"], stats.parity as i64);
    assert!(data["totalActualSize"].as_i64().unwrap() > 0);
    assert_eq!(
        data["totalActualSize"], data["oneRegionActualSize"],
        "one game, one region: the two actual sizes weigh the same"
    );

    // when the store moves out from under the system, its ORE with it, and
    // the system gets an empty store of its own: the ORE now lives beneath
    // a store the system no longer names, which the preload cannot answer
    // for and the sizes must read off disk
    let original_size = data["totalActualSize"].as_i64().unwrap();
    let romfiles = find_romfiles(&mut env.connection).await;
    let ore = romfiles
        .iter()
        .find(|romfile| romfile.path.ends_with(".ore"))
        .unwrap()
        .path
        .clone();
    let ore_name = Path::new(&ore)
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    tokio::fs::rename(&store, env.rom_directory.join(ore_rs::STORE_DIR))
        .await
        .unwrap();
    tokio::fs::rename(
        env.rom_directory.join(&ore),
        env.rom_directory.join(&ore_name),
    )
    .await
    .unwrap();
    sqlx::query(AssertSqlSafe(
        format!("UPDATE romfiles SET path = '{ore_name}';").as_str(),
    ))
    .execute(&mut *env.connection)
    .await
    .unwrap();
    ore_rs::create_store(&store, &ore_rs::StoreOptions::default()).unwrap();

    // and a fresh request asks the sizes of the system again, through a
    // loader that has never seen it: its preload is the system's new, empty
    // store, and the ORE's own is the moved one
    let store_index_loader = StoreIndexLoader::new(env.pool.clone());
    let counted = store_index_loader.clone();
    let (sse_tx, _) = broadcast::channel::<SseMessage>(100);
    let schema = build_schema(env.pool.clone(), sse_tx, store_index_loader);
    let response = schema.execute("{ totalActualSize(systemId: 1) }").await;
    assert!(response.errors.is_empty(), "{:?}", response.errors);

    // then the ORE still weighs its share of the store before it: the same
    // weight as when the system named that store, read off disk rather
    // than counted as its file, and never from the mismatched preload
    let data = response.data.into_json().unwrap();
    assert_eq!(
        data["totalActualSize"].as_i64().unwrap(),
        original_size,
        "the ORE followed its store: the same share, read off disk"
    );
    assert_eq!(
        counted.hits(),
        0,
        "a store the system does not name is weighed off disk, not from its preload"
    );
}
