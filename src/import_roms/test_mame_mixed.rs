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
    set_rom_directory(PathBuf::from(rom_directory.path()));
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

    let matches = import_dats::subcommand()
        .get_matches_from(&["import-dats", "tests/Test System (20241230) (MAME Mixed).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom");
    fs::copy(
        test_directory.join("Test Game (USA, Europe).rom"),
        &romfile_path.as_os_str().to_str().unwrap(),
    )
    .await
    .unwrap();
    let chd_romfile_path = tmp_directory.join("Test Game (USA, Europe) (RDSK).chd");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (RDSK).chd"),
        &chd_romfile_path.as_os_str().to_str().unwrap(),
    )
    .await
    .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    // when
    import_other(
        &mut connection,
        &progress_bar,
        &Some(&system),
        &None,
        &HashSet::new(),
        CommonRomfile::from_path(&romfile_path).unwrap(),
        true,
        false,
        &None,
    )
    .await
    .unwrap();
    import_chd(
        &mut connection,
        &progress_bar,
        &Some(&system),
        &HashSet::new(),
        CommonRomfile::from_path(&chd_romfile_path).unwrap(),
        true,
        false,
        &None,
    )
    .await
    .unwrap();

    // then
    let mut roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 2);
    let mut romfiles = find_romfiles(&mut connection).await;
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
    assert_eq!(game.name, "Test Game (USA, Europe)");
    assert_eq!(game.system_id, system.id);

    let rom = roms.pop().unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe).rom");
    assert_eq!(rom.game_id, game.id);

    let romfile = romfiles.pop().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe)")
            .join("Test Game (USA, Europe).rom")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(rom_directory.path().join(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));

    let rom = roms.pop().unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe)");
    assert_eq!(rom.game_id, game.id);

    let romfile = romfiles.pop().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe)")
            .join("Test Game (USA, Europe).chd")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(rom_directory.path().join(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));
}