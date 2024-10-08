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

    let system_directory = tmp_directory.join("Test System");
    fs::create_dir(&system_directory).await.unwrap();

    let romfile_path = system_directory.join("Test Game (USA, Europe).rom");
    fs::copy(
        test_directory.join("Test Game (USA, Europe).rom"),
        &romfile_path.as_os_str().to_str().unwrap(),
    )
    .await
    .unwrap();

    let dat_directory = tmp_directory.join("DAT");
    let dat_path = dat_directory.join("Test System (42).dat");

    let matches = import_dats::subcommand()
        .get_matches_from(&["import-dats", dat_path.as_os_str().to_str().unwrap()]);

    // when
    create_dat(
        &mut connection,
        &progress_bar,
        &system_directory,
        Some(&dat_directory),
        None,
        None,
        Some(&String::from("42")),
        None,
        None,
    )
    .await
    .unwrap();
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then
    let systems = find_systems(&mut connection).await;
    assert_eq!(systems.len(), 1);
    let system = systems.first().unwrap();
    assert_eq!(system.name, "Test System");
    let games = find_games_by_system_id(&mut connection, system.id).await;
    assert_eq!(games.len(), 1);
    let game = games.first().unwrap();
    assert_eq!(game.name, "Test Game (USA, Europe)");
    let roms = find_roms_without_romfile_by_game_ids(&mut connection, &[game.id]).await;
    assert_eq!(roms.len(), 1);
    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe).rom");
    assert_eq!(rom.game_id, game.id);
}
