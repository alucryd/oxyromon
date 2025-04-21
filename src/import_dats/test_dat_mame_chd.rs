use super::super::database::*;
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

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let dat_path = test_directory.join("Test System (20241228) (MAME CHD).dat");
    let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

    // when
    import_dat(
        &mut connection,
        &progress_bar,
        &datfile_xml,
        &detector_xml,
        None,
        false,
    )
    .await
    .unwrap();

    // then
    let systems = find_systems(&mut connection).await;
    assert_eq!(systems.len(), 1);

    let system = systems.first().unwrap();
    assert_eq!(system.name, "Test System (MAME CHD)");
    assert_eq!(system.arcade, true);

    assert_eq!(find_games(&mut connection).await.len(), 1);
    assert_eq!(find_roms(&mut connection).await.len(), 1);
}
