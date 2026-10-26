use super::super::config::{MUTEX, set_rom_directory, set_tmp_directory};
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

    // a lossless WUA, imported with its update partition
    let staging = tmp_directory.join("staging");
    fs::create_dir(&staging).await.unwrap();
    let keys = wux_rs::load_keys(&test_directory.join("Test Game (USA).keys.txt")).unwrap();
    wux_rs::to_wua(
        &[&test_directory.join("Test Game (USA).wux")],
        &staging.join("Test Game (USA).wua"),
        &keys,
        Some(&staging.join(STORE_DIR)),
        &mut |_| {},
    )
    .unwrap();
    let matches = import_roms::subcommand()
        .get_matches_from(["import-roms", staging.as_os_str().to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();
    let wua = system_directory.join("Test Game (USA).wua");
    let store = system_directory.join(STORE_DIR);
    let entry = store.join(&wux_rs::store_entries(&wua).unwrap()[0]);
    // and an update partition no WUA needs any more
    let stray = store.join("0000000000000000000000000000000000000000.zst");
    fs::write(&stray, b"stray").await.unwrap();
    // but the WUA is cut short, so what it needs can't be read
    let length = fs::metadata(&wua).await.unwrap().len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&wua)
        .unwrap()
        .set_len(length / 2)
        .unwrap();

    // when
    purge_foreign_romfiles(&mut connection, &progress_bar, true)
        .await
        .unwrap();

    // then every update partition is kept
    assert!(wua.is_file());
    assert!(entry.is_file());
    assert!(stray.is_file());
}
