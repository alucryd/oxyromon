use super::super::config::*;
use super::super::database::*;
use super::{import_dat, main as import_dats_main, parse_dat, subcommand};
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

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    // first import the system from a plain DAT so it exists in the database
    let dat_path = test_directory.join("Test System (20200721).dat");
    let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

    import_dat(
        &mut connection,
        &progress_bar,
        &datfile_xml,
        &detector_xml,
        None,
        None,
        false,
    )
    .await
    .unwrap();

    let systems = find_systems(&mut connection).await;
    assert_eq!(systems.len(), 1);
    assert_eq!(systems.first().unwrap().version, "20200721");
    assert_eq!(find_games(&mut connection).await.len(), 6);
    assert_eq!(find_roms(&mut connection).await.len(), 8);

    // when: import the ZIP with --update flag; the system is known so it should be updated
    let zip_path = test_directory.join("Test System (20210401).zip");

    let matches = subcommand().get_matches_from([
        "import-dats",
        "-u",
        zip_path.as_os_str().to_str().unwrap(),
    ]);

    import_dats_main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then: the system should have been updated to the newer version
    let systems = find_systems(&mut connection).await;
    assert_eq!(systems.len(), 1);

    let system = systems.first().unwrap();
    assert_eq!(system.name, "Test System");
    assert_eq!(system.version, "20210401");

    assert_eq!(find_games(&mut connection).await.len(), 3);
    assert_eq!(find_roms(&mut connection).await.len(), 3);
}
