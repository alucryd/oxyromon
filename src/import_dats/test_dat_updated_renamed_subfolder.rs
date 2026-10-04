use super::super::config::*;
use super::super::import_roms::UnattendedMode;
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
    set_string(&mut connection, "REGIONS_ALL_SUBFOLDERS", "alpha", None).await;

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

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom");
    fs::copy(
        test_directory.join("Test Game (USA, Europe).rom"),
        &romfile_path,
    )
    .await
    .unwrap();
    import_rom(
        &mut connection,
        &progress_bar,
        &Some(&system),
        &None,
        &romfile_path,
        false,
        false,
        false,
        UnattendedMode::Skip,
        false,
    )
    .await
    .unwrap();

    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(
        romfiles[0].as_common(&mut connection).await.unwrap().path,
        system_directory
            .join("T")
            .join("Test Game (USA, Europe).rom")
    );

    let dat_path = test_directory.join("Test System (20200722) (Renamed Game).dat");
    let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

    // when
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

    // then
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    assert_eq!(
        romfiles[0].as_common(&mut connection).await.unwrap().path,
        system_directory
            .join("R")
            .join("Renamed Game (USA, Europe).rom")
    );
    assert!(!system_directory.join("T").exists());
    assert!(system_directory.is_dir());
}
