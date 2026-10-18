use super::super::config::*;
use super::*;
use std::path::PathBuf;
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

    let dat_path = test_directory.join("Test System (20200721).dat");
    let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();
    let newer_dat_path = test_directory.join("Test System (20200722) (Renamed Game).dat");
    let (newer_datfile_xml, newer_detector_xml) = parse_dat(&progress_bar, &newer_dat_path, false)
        .await
        .unwrap();

    // when: a DAT listing games, imported as arcade
    import_dat(
        &mut connection,
        &progress_bar,
        &datfile_xml,
        &detector_xml,
        None,
        None,
        false,
        true,
    )
    .await
    .unwrap();

    // then: arcade, so no regions are parsed from the game names
    let system = find_systems(&mut connection).await.remove(0);
    assert!(system.arcade);
    let games = find_games(&mut connection).await;
    assert_eq!(games.len(), 6);
    assert!(games.iter().all(|game| game.regions.is_empty()));

    // when: an update without the flag
    import_dat(
        &mut connection,
        &progress_bar,
        &newer_datfile_xml,
        &newer_detector_xml,
        None,
        None,
        false,
        false,
    )
    .await
    .unwrap();

    // then: the system stays arcade
    assert!(find_systems(&mut connection).await.remove(0).arcade);

    // when: a forced import without the flag
    import_dat(
        &mut connection,
        &progress_bar,
        &newer_datfile_xml,
        &newer_detector_xml,
        None,
        None,
        true,
        false,
    )
    .await
    .unwrap();

    // then: the flag applies as given, and regions are back
    assert!(!find_systems(&mut connection).await.remove(0).arcade);
    assert!(
        find_games(&mut connection)
            .await
            .iter()
            .any(|game| !game.regions.is_empty())
    );
}
