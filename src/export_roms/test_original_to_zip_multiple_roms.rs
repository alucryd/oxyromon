use super::super::database::*;
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

    let destination_directory = tmp_directory.join("destination");
    create_directory(&progress_bar, &destination_directory, true)
        .await
        .unwrap();

    // when
    to_archive(
        &mut connection,
        &progress_bar,
        &destination_directory,
        &system,
        games_by_id,
        roms_by_game_id,
        romfiles_by_id,
        sevenzip::ArchiveType::Zip,
        &None,
        false,
    )
    .await
    .unwrap();

    // then
    assert!(destination_directory
        .join("Test Game (USA, Europe) (DLC).zip")
        .is_file());
}
