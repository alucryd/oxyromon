use super::super::database::*;
use super::super::import_dats;
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

    let matches = import_dats::subcommand()
        .get_matches_from(&["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Headered).rom");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (Headered).rom"),
        &romfile_path.as_os_str().to_str().unwrap(),
    )
    .await
    .unwrap();

    let system = find_systems(&mut connection).await.remove(0);

    // when
    import_other(
        &mut connection,
        &progress_bar,
        &Some(&system),
        &None,
        &HashSet::new(),
        CommonRomfile::from_path(&romfile_path).unwrap(),
        false,
        true,
        UnattendedMode::Skip,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 0);
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 0);

    assert!(
        !rom_directory
            .path()
            .join("Trash")
            .join("Test Game (USA, Europe) (Headered).rom")
            .is_file()
    );
}
