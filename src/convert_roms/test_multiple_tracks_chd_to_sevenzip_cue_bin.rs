use super::super::database::*;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};
use tokio::fs;

#[tokio::test]
async fn test() {
    if let Ok(version) = chdman::get_version().await {
        if version.as_str().cmp(chdman::MIN_SPLITBIN_VERSION) == Ordering::Less {
            return;
        }
    }

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

    let cue_romfile_path = tmp_directory.join("Test Game (USA, Europe) (Multiple Tracks).cue");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (Multiple Tracks).cue"),
        &cue_romfile_path,
    )
    .await
    .unwrap();
    let chd_romfile_path = tmp_directory.join("Test Game (USA, Europe) (Multiple Tracks).chd");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (Multiple Tracks).chd"),
        &chd_romfile_path,
    )
    .await
    .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    let matches = import_roms::subcommand().get_matches_from(&[
        "import-roms",
        chd_romfile_path.as_os_str().to_str().unwrap(),
        cue_romfile_path.as_os_str().to_str().unwrap(),
    ]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let games = find_full_games_by_system_id(&mut connection, system.id).await;
    let roms = find_roms_with_romfile_by_game_ids(&mut connection, &[games[0].id]).await;
    let games_by_id: HashMap<i64, Game> = games.into_iter().map(|game| (game.id, game)).collect();
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    for rom in &roms {
        let romfile = find_romfile_by_id(&mut connection, rom.romfile_id.unwrap()).await;
        romfiles_by_id.insert(romfile.id, romfile);
    }
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);

    // when
    to_archive(
        &mut connection,
        &progress_bar,
        &system,
        games_by_id,
        roms_by_game_id,
        romfiles_by_id,
        sevenzip::ArchiveType::Sevenzip,
        false,
        false,
        true,
        &None,
        false,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 3);
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);

    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe) (CUE BIN).7z")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(rom_directory.path().join(&romfile.path).is_file());

    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe) (CUE BIN) (Track 01).bin");
    assert_eq!(rom.romfile_id, Some(romfile.id));
    let rom = roms.get(1).unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe) (CUE BIN) (Track 02).bin");
    assert_eq!(rom.romfile_id, Some(romfile.id));
    let rom = roms.get(2).unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe) (CUE BIN).cue");
    assert_eq!(rom.romfile_id, Some(romfile.id));

    let archive_romfiles = romfile
        .as_common(&mut connection)
        .await
        .unwrap()
        .as_archive(&progress_bar, None)
        .await
        .unwrap();
    assert_eq!(archive_romfiles.len(), 3);
}
