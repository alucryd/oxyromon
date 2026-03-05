use super::super::config::*;
use super::super::database::*;
use super::{main as import_dats_main, subcommand};
use indicatif::ProgressBar;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};

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
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    // no system has been imported yet; the database is empty
    let systems = find_systems(&mut connection).await;
    assert_eq!(systems.len(), 0);

    // when: import the ZIP with --update flag; the system is unknown so it should be skipped
    let zip_path = test_directory.join("Test System (20210401).zip");

    let matches = subcommand().get_matches_from(&[
        "import-dats",
        "-u",
        zip_path.as_os_str().to_str().unwrap(),
    ]);

    import_dats_main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then: no system should have been created
    let systems = find_systems(&mut connection).await;
    assert_eq!(systems.len(), 0);
    assert_eq!(find_games(&mut connection).await.len(), 0);
    assert_eq!(find_roms(&mut connection).await.len(), 0);
}
