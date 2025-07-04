use super::super::chdman;
use super::super::config::*;
use super::super::database::*;
use super::*;
use std::cmp::Ordering;
use std::path::PathBuf;
use tempfile::{NamedTempFile, TempDir};
use tokio::fs;

#[tokio::test]
async fn test() {
    if let Ok(version) = chdman::get_version().await {
        if version.as_str().cmp(chdman::MIN_SPLITBIN_VERSION) == Ordering::Less {
            return;
        }
    }

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

    let cue_romfile_path = tmp_directory.join("Test Game (USA, Europe) (Multiple Tracks).cue");
    fs::copy(
        test_directory.join(&cue_romfile_path.file_name().unwrap().to_str().unwrap()),
        &cue_romfile_path.as_os_str().to_str().unwrap(),
    )
    .await
    .unwrap();
    let chd_romfile_path = tmp_directory.join("Test Game (USA, Europe) (Multiple Tracks).chd");
    fs::copy(
        test_directory.join(&chd_romfile_path.file_name().unwrap().to_str().unwrap()),
        &chd_romfile_path.as_os_str().to_str().unwrap(),
    )
    .await
    .unwrap();
    import_rom(
        &mut connection,
        &progress_bar,
        &Some(&system),
        &None,
        &cue_romfile_path,
        true,
        false,
        true,
        false,
        false,
    )
    .await
    .unwrap();
    import_rom(
        &mut connection,
        &progress_bar,
        &Some(&system),
        &None,
        &chd_romfile_path,
        true,
        false,
        true,
        false,
        false,
    )
    .await
    .unwrap();

    let dat_path =
        test_directory.join("Test System (20240626) (Multiple Orphan Roms Mismatch).dat");
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
    assert_eq!(romfiles.len(), 2);

    let romfile = romfiles.first().unwrap();
    assert!(romfile.path.contains("/Trash/"));
}
