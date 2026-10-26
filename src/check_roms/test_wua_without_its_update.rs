use super::super::config::*;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20261025) (WUD).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // a lossless WUA, its update partition in the store beside it
    let staging = tmp_directory.join("staging");
    fs::create_dir(&staging).await.unwrap();
    let wua_path = staging.join("Test Game (USA).wua");
    let keys = wux_rs::load_keys(&test_directory.join("Test Game (USA).keys.txt")).unwrap();
    wux_rs::to_wua(
        &[&test_directory.join("Test Game (USA).wux")],
        &wua_path,
        &keys,
        Some(&staging.join(wux_rs::STORE_DIR)),
        &mut |_| {},
    )
    .unwrap();

    let system = find_systems(&mut connection).await.remove(0);

    let matches = import_roms::subcommand()
        .get_matches_from(["import-roms", staging.as_os_str().to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // the update partition, gone from the system's store: the WUA can't be
    // rebuilt, so it fails its check
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();
    for entry in wux_rs::store_entries(&system_directory.join("Test Game (USA).wua")).unwrap() {
        fs::remove_file(system_directory.join(wux_rs::STORE_DIR).join(entry))
            .await
            .unwrap();
    }

    let games = find_full_games_by_system_id(&mut connection, system.id).await;

    // when
    check_system(&mut connection, &progress_bar, &system, games, false)
        .await
        .unwrap();

    // then
    let mut romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);

    let romfile = romfiles.remove(0);
    assert!(romfile.path.contains("/Trash/"));
    assert!(&rom_directory.path().join(&romfile.path).is_file());
}
