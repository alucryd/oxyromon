use super::super::config::*;
use super::super::database::*;
use super::super::import_dats;
use super::super::import_irds;
use super::*;
use std::env;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    // TODO: Audit that the environment access only happens in single-threaded code.
    unsafe {
        env::set_var(
            "PATH",
            format!(
                "{}:{}",
                test_directory.as_os_str().to_str().unwrap(),
                env::var("PATH").unwrap()
            ),
        )
    };
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand()
        .get_matches_from(&["import-dats", "tests/Test System (20240704) (IRD).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let matches =
        import_irds::subcommand().get_matches_from(&["import-irds", "tests/Test Game (USA).ird"]);
    import_irds::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let game = find_games(&mut connection).await.remove(0);

    let roms_before = find_roms(&mut connection).await;
    assert_eq!(roms_before.len(), 208);

    // Refetch game to get updated jbfolder status
    let game = find_game_by_id(&mut connection, game.id).await;
    assert!(game.jbfolder);

    // when
    purge_ird(&mut connection, &progress_bar, &game)
        .await
        .unwrap();

    // then
    let roms_after = find_roms(&mut connection).await;
    assert_eq!(roms_after.len(), 1); // Only the parent rom should remain

    let updated_game = find_game_by_id(&mut connection, game.id).await;
    assert!(!updated_game.jbfolder);
}
