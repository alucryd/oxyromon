use super::super::database::*;
use super::super::import_dats;
use super::*;
use async_std::fs;
use async_std::path::PathBuf;
use tempfile::{NamedTempFile, TempDir};

#[async_std::test]
async fn test() {
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

    let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Full).7z");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (Full).7z"),
        &romfile_path.as_os_str().to_str().unwrap(),
    )
    .await
    .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &progress_bar, &system)
        .await
        .unwrap();

    // when
    import_archive(
        &mut connection,
        &progress_bar,
        &system_directory,
        &system,
        &None,
        &romfile_path,
        romfile_path.extension().unwrap().to_str().unwrap(),
        &HashAlgorithm::Crc,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 3);
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    let games = find_games_by_ids(
        &mut connection,
        roms.iter()
            .map(|rom| rom.game_id)
            .collect::<Vec<i64>>()
            .as_slice(),
    )
    .await;
    assert_eq!(games.len(), 1);

    let game = games.get(0).unwrap();
    assert_eq!(game.name, "Test Game (USA, Europe) (CUE BIN)");
    assert_eq!(game.system_id, system.id);

    let romfile = romfiles.get(0).unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe) (CUE BIN).7z")
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(Path::new(&romfile.path).is_file().await);

    let rom = roms.get(0).unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe) (Track 01).bin");
    assert_eq!(rom.game_id, game.id);
    assert_eq!(rom.romfile_id, Some(romfile.id));

    let rom = roms.get(1).unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe) (Track 02).bin");
    assert_eq!(rom.game_id, game.id);
    assert_eq!(rom.romfile_id, Some(romfile.id));

    let rom = roms.get(2).unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe).cue");
    assert_eq!(rom.game_id, game.id);
    assert_eq!(rom.romfile_id, Some(romfile.id));
}
