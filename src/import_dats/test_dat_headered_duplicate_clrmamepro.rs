use super::super::config::*;
use super::super::database::*;
use super::*;
use async_std::path::PathBuf;
use tempfile::{NamedTempFile, TempDir};

#[async_std::test]
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
    set_tmp_directory(PathBuf::from(tmp_directory.path()));

    let dat_path =
        test_directory.join("Test System (20220430) (Headered) (Duplicate Clrmamepro).dat");
    let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

    // when
    import_dat(
        &mut connection,
        &progress_bar,
        &datfile_xml,
        &detector_xml,
        false,
        false,
    )
    .await
    .unwrap();

    // then
    let systems = find_systems(&mut connection).await;
    assert_eq!(systems.len(), 1);

    let system = systems.get(0).unwrap();
    assert_eq!(system.name, "Test System (Headered)");

    assert!(find_header_by_system_id(&mut connection, system.id)
        .await
        .is_some());

    assert_eq!(find_games(&mut connection).await.len(), 1);
    assert_eq!(find_roms(&mut connection).await.len(), 1);
}
