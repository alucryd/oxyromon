use super::super::database::*;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
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
        .get_matches_from(&["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Multiple Tracks).cue");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (Multiple Tracks).cue"),
        &romfile_path,
    )
    .await
    .unwrap();
    let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Multiple Tracks).chd");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (Multiple Tracks).chd"),
        &romfile_path,
    )
    .await
    .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    let matches = import_roms::subcommand()
        .get_matches_from(&["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let games = find_games_with_romfiles_by_system_id(&mut connection, system.id).await;
    let roms = find_roms_with_romfile_by_game_ids(&mut connection, &[games[0].id]).await;
    let games_by_id: HashMap<i64, Game> = games.into_iter().map(|game| (game.id, game)).collect();
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    for rom in &roms {
        let romfile = find_romfile_by_id(&mut connection, rom.romfile_id.unwrap()).await;
        romfiles_by_id.insert(romfile.id, romfile);
    }
    let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);

    // when
    to_archive(
        &mut connection,
        &progress_bar,
        sevenzip::ArchiveType::Sevenzip,
        &system,
        roms_by_game_id,
        games_by_id,
        romfiles_by_id,
        false,
        1,
        false,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 3);
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);

    let romfile = romfiles.get(0).unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe) (CUE BIN).7z")
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(Path::new(&romfile.path).is_file());

    let rom = roms.get(0).unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe) (Track 01).bin");
    assert_eq!(rom.romfile_id, Some(romfile.id));
    let rom = roms.get(1).unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe) (Track 02).bin");
    assert_eq!(rom.romfile_id, Some(romfile.id));
    let rom = roms.get(2).unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe).cue");
    assert_eq!(rom.romfile_id, Some(romfile.id));

    let sevenzip_infos = sevenzip::parse_archive(&progress_bar, &romfile.path)
        .await
        .unwrap();
    assert_eq!(sevenzip_infos.len(), 3);
}
