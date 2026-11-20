//! The sizes of a system whose games live in a store, as one GraphQL
//! request asks for them: the store's index is read once, whatever the
//! number of fields that need it.

use super::super::config::{MUTEX, set_bool, set_rom_directory, set_string, set_tmp_directory};
use super::super::convert_roms;
use super::super::import_dats;
use super::super::import_roms;
use super::super::sort_roms;
use super::super::util::get_system_directory;
use super::*;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = crate::config::TestDatabase::new();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20261114) (Game Boy).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_path = tmp_directory.join("Test Game (USA, Europe).gb");
    tokio::fs::copy(
        test_directory.join("Test Game (USA, Europe).gb"),
        &romfile_path,
    )
    .await
    .unwrap();
    let matches = import_roms::subcommand()
        .get_matches_from(["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let matches =
        sort_roms::subcommand().get_matches_from(["sort-roms", "-a", "-y", "-o", "US", "-r", "JP"]);
    sort_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // and the game's ORE in the system's store
    let system = find_systems(&mut connection).await.remove(0);
    set_bool(&mut connection, "ORE_STORE", true, Some(system.id)).await;
    set_string(&mut connection, "ORE_STORE_BATCH", "game", Some(system.id)).await;
    let matches =
        convert_roms::subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a", "-c"]);
    convert_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // when one request asks for every size of the system
    let store_index_loader = StoreIndexLoader::new(pool.clone());
    let counted = store_index_loader.clone();
    let (sse_tx, _) = broadcast::channel::<SseMessage>(100);
    let schema = build_schema(pool.clone(), sse_tx, store_index_loader);
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
    let store = get_system_directory(&mut connection, &system)
        .await
        .unwrap()
        .join(ore_rs::STORE_DIR);
    let stats = ore_rs::store_stats(&store).unwrap();
    assert_eq!(data["storeDedupSaved"], stats.dedup_saved as i64);
    assert_eq!(data["storeParityCost"], stats.parity as i64);
    assert!(data["totalActualSize"].as_i64().unwrap() > 0);
    assert_eq!(
        data["totalActualSize"], data["oneRegionActualSize"],
        "one game, one region: the two actual sizes weigh the same"
    );
}
