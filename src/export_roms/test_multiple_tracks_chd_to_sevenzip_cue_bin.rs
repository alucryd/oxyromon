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
        sevenzip::ArchiveType::Sevenzip,
        &None,
        false,
    )
    .await
    .unwrap();

    // then
    assert!(
        destination_directory
            .join("Test Game (USA, Europe) (CUE BIN).7z")
            .is_file()
    );
}
