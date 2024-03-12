use super::super::database::*;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::env;
use std::path::Path;
use tempfile::{NamedTempFile, TempDir};
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    env::set_var(
        "PATH",
        format!(
            "{}:{}",
            test_directory.as_os_str().to_str().unwrap(),
            env::var("PATH").unwrap()
        ),
    );
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    set_rom_directory(PathBuf::from(rom_directory.path()));
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

    let matches = import_dats::subcommand().get_matches_from(&[
        "import-dats",
        "tests/Test System (20230105) (Multiple Discs).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);

    let romfile_name = "Test Game (USA, Europe) (Disc 1).iso";
    let romfile_path = tmp_directory.join(&romfile_name);
    fs::copy(test_directory.join(&romfile_name), &romfile_path)
        .await
        .unwrap();
    let matches = import_roms::subcommand()
        .get_matches_from(&["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // when
    process_system(&mut connection, &progress_bar, &system)
        .await
        .unwrap();

    // then
    let games = find_games_with_romfiles_by_system_id(&mut connection, system.id).await;
    assert_eq!(games.len(), 1);

    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 1);

    let playlist_id = games.first().unwrap().playlist_id;
    assert!(playlist_id.is_none());
}
