use super::super::config::*;
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

    let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom");
    fs::copy(
        test_directory.join("Test Game (USA, Europe).rom"),
        &romfile_path,
    )
    .await
    .unwrap();
    let matches = import_roms::subcommand()
        .get_matches_from(["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // another game's ROM stored in the same file, as in a merged set
    let system = find_systems(&mut connection).await.remove(0);
    let games = find_games_by_system_id(&mut connection, system.id).await;
    let game = games
        .iter()
        .find(|game| game.name == "Test Game (USA, Europe)")
        .unwrap();
    let other_game = games
        .iter()
        .find(|game| game.name == "Test Game (Japan)")
        .unwrap();
    let romfile_id = find_roms_with_romfile_by_game_id(&mut connection, game.id)
        .await
        .remove(0)
        .romfile_id;
    let other_rom = find_roms_by_game_id_no_parents(&mut connection, other_game.id)
        .await
        .remove(0);
    update_rom_romfile(&mut connection, other_rom.id, romfile_id).await;

    // when
    purge_games(&mut connection, &progress_bar, std::slice::from_ref(game))
        .await
        .unwrap();

    // then
    let romfile = find_romfile_by_id(&mut connection, romfile_id.unwrap()).await;
    assert!(!romfile.path.contains("Trash"));
    assert_eq!(
        find_roms_by_romfile_id(&mut connection, romfile.id)
            .await
            .len(),
        2
    );
}
