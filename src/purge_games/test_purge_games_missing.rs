use super::super::config::*;
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

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_name = "Test Game (USA, Europe).rom";
    let romfile_path = tmp_directory.join(romfile_name);
    fs::copy(test_directory.join(romfile_name), &romfile_path)
        .await
        .unwrap();
    let matches = import_roms::subcommand()
        .get_matches_from(["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // the file was deleted by hand
    let system = find_systems(&mut connection).await.remove(0);
    let game = find_games_by_system_id(&mut connection, system.id)
        .await
        .into_iter()
        .find(|game| game.name == "Test Game (USA, Europe)")
        .unwrap();
    let romfile_id = find_roms_with_romfile_by_game_id(&mut connection, game.id)
        .await
        .remove(0)
        .romfile_id
        .unwrap();
    let romfile = find_romfile_by_id(&mut connection, romfile_id).await;
    fs::remove_file(romfile.as_common(&mut connection).await.unwrap().path)
        .await
        .unwrap();

    // when
    purge_games(&mut connection, &progress_bar, std::slice::from_ref(&game))
        .await
        .unwrap();

    // then its record is gone, and the game is missing
    assert!(find_romfiles(&mut connection).await.is_empty());
    assert!(
        find_roms_with_romfile_by_game_id(&mut connection, game.id)
            .await
            .is_empty()
    );
    let game = find_game_by_id(&mut connection, game.id).await;
    assert_eq!(game.completion, 0);
}
