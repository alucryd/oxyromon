use super::super::database::*;
use super::super::import_dats;
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

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand()
        .get_matches_from(&["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Partial).7z");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (Partial).7z"),
        &romfile_path.as_os_str().to_str().unwrap(),
    )
    .await
    .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    // when
    import_archive(
        &mut connection,
        &progress_bar,
        &Some(&system),
        &None,
        &HashSet::new(),
        CommonRomfile::from_path(&romfile_path).unwrap(),
        romfile_path.extension().unwrap().to_str().unwrap(),
        true,
        false,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 2);
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 2);
    let games = find_games_by_ids(
        &mut connection,
        roms.iter()
            .map(|rom| rom.game_id)
            .collect::<Vec<i64>>()
            .as_slice(),
    )
    .await;
    assert_eq!(games.len(), 1);

    let game = games.first().unwrap();
    assert_eq!(game.name, "Test Game (USA, Europe) (CUE BIN)");
    assert_eq!(game.system_id, system.id);

    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe) (CUE BIN) (Track 01).bin")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(rom_directory.path().join(&romfile.path).is_file());

    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe) (CUE BIN) (Track 01).bin");
    assert_eq!(rom.game_id, game.id);
    assert_eq!(rom.romfile_id, Some(romfile.id));

    let romfile = romfiles.get(1).unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe) (CUE BIN).cue")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(rom_directory.path().join(&romfile.path).is_file());

    let rom = roms.get(1).unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe) (CUE BIN).cue");
    assert_eq!(rom.game_id, game.id);
    assert_eq!(rom.romfile_id, Some(romfile.id));
}
