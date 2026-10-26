use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::path::PathBuf;
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
    let staging_store = staging.join(wux_rs::STORE_DIR);
    let keys = wux_rs::load_keys(&test_directory.join("Test Game (USA).keys.txt")).unwrap();
    wux_rs::to_wua(
        &[&test_directory.join("Test Game (USA).wux")],
        &wua_path,
        &keys,
        Some(&staging_store),
        &mut |_| {},
    )
    .unwrap();
    let entries = wux_rs::store_entries(&wua_path).unwrap();
    assert_eq!(entries.len(), 1);

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    // when
    let matches = import_roms::subcommand()
        .get_matches_from(["import-roms", staging.as_os_str().to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then the WUA is matched by the disc it holds, and its update partition
    // copied into the system's store, the staging store left as it was
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 1);
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA).iso");
    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA).wua")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert_eq!(rom.romfile_id, Some(romfile.id));
    assert!(
        system_directory
            .join(wux_rs::STORE_DIR)
            .join(&entries[0])
            .is_file()
    );
    assert!(staging_store.join(&entries[0]).is_file());
}
