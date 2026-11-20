use super::super::import_dats;
use super::*;
use std::path::PathBuf;
use tempfile::TempDir;
use tokio::fs;

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

    // Renamed Game and Another Game share their Track 02
    let matches = import_dats::subcommand().get_matches_from([
        "import-dats",
        "tests/Test System (20240627) (Renamed Tracks Shared).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    // named neither as a ROM nor as a game, but closest to Renamed Game's Track 02
    let romfile_path = tmp_directory.join("renamed game (track 02).bin");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (CUE BIN) (Track 02).bin"),
        &romfile_path,
    )
    .await
    .unwrap();

    // when
    let matches = subcommand().get_matches_from([
        "import-roms",
        "-u",
        "first",
        romfile_path.as_os_str().to_str().unwrap(),
    ]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then it is Renamed Game's
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(
        roms.iter()
            .map(|rom| rom.name.as_str())
            .collect::<Vec<&str>>(),
        ["Renamed Game (USA, Europe) (Track 02).bin"]
    );
    let romfile = find_romfile_by_id(&mut connection, roms[0].romfile_id.unwrap()).await;
    assert_eq!(
        romfile.path,
        system_directory
            .join("Renamed Game (USA, Europe) (Track 02).bin")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
}
