use super::super::database::*;
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

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    set_rom_directory(PathBuf::from(rom_directory.path()));
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

    let matches = import_dats::subcommand()
        .get_matches_from(&["import-dats", "tests/Test System (20230527) (PSN).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let mut romfile_paths: Vec<PathBuf> = Vec::new();
    let romfile_path = tmp_directory.join("UP0001-BLUS00001.pkg");
    fs::copy(test_directory.join("UP0001-BLUS00001.pkg"), &romfile_path)
        .await
        .unwrap();
    romfile_paths.push(romfile_path);
    let romfile_path = tmp_directory.join("prfgmHWxGNxsfJ.rap");
    fs::copy(test_directory.join("prfgmHWxGNxsfJ.rap"), &romfile_path)
        .await
        .unwrap();
    romfile_paths.push(romfile_path);

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    for romfile_path in romfile_paths {
        let matches = import_roms::subcommand()
            .get_matches_from(&["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
        import_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();
    }

    let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
    let mut games_by_id: HashMap<i64, Game> = HashMap::new();
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    for rom in &roms {
        let romfile = find_romfile_by_id(&mut connection, rom.romfile_id.unwrap()).await;
        romfiles_by_id.insert(romfile.id, romfile);
    }
    let game = find_game_by_id(&mut connection, roms[0].game_id).await;
    games_by_id.insert(roms[0].game_id, game);
    roms_by_game_id.insert(roms[0].game_id, roms);

    // when
    to_archive(
        &mut connection,
        &progress_bar,
        sevenzip::ArchiveType::Zip,
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
    assert_eq!(roms.len(), 2);
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);

    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe) (DLC).zip")
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(Path::new(&romfile.path).is_file());

    let archive_romfiles = sevenzip::parse(&progress_bar, &romfile.path).await.unwrap();
    assert_eq!(archive_romfiles.len(), 2);

    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "UP0001-BLUS00001.pkg");
    assert_eq!(rom.romfile_id, Some(romfile.id));

    let rom = roms.get(1).unwrap();
    assert_eq!(rom.name, "prfgmHWxGNxsfJ.rap");
    assert_eq!(rom.romfile_id, Some(romfile.id));
}
