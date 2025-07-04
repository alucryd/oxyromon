use super::super::config::*;
use super::super::database::*;
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
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

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

    let romfile_names = vec!["Test Game (USA, Europe) (Full).7z"];
    for romfile_name in romfile_names {
        let romfile_path = tmp_directory.join(romfile_name);
        fs::copy(
            test_directory.join(romfile_name),
            &romfile_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();
        import_rom(
            &mut connection,
            &progress_bar,
            &Some(&system),
            &None,
            &romfile_path,
            true,
            false,
            true,
            false,
            false,
        )
        .await
        .unwrap();
    }

    let dat_path = test_directory.join("Test System (20240626) (Multiple Orphan Roms).dat");
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
    let systems = find_systems(&mut connection).await;
    assert_eq!(systems.len(), 1);

    let system = systems.first().unwrap();
    assert_eq!(system.name, "Test System");

    let games = find_games(&mut connection).await;
    let roms = find_roms(&mut connection).await;
    let romfiles = find_romfiles(&mut connection).await;

    assert_eq!(games.len(), 1);
    assert_eq!(roms.len(), 3);
    assert_eq!(romfiles.len(), 1);

    let romfile = romfiles.first().unwrap();
    assert!(!romfile.path.contains("/Trash/"));
}
