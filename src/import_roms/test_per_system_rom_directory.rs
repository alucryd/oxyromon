use super::super::database::*;
use super::super::import_dats;
use super::*;
use std::collections::HashSet;
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

    let global_rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(global_rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand().get_matches_from([
        "import-dats",
        "tests/Test System (20240229) (Single Track).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let matches = import_dats::subcommand().get_matches_from([
        "import-dats",
        "tests/Test System (20230105) (Multiple Discs).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let mut systems = find_systems(&mut connection).await;
    assert_eq!(systems.len(), 2);
    // find_systems returns systems ordered by name:
    // "Test System" < "Test System (Multiple Discs)"
    let system_single_track = systems.remove(0);
    let system_multiple_discs = systems.remove(0);

    let rom_directory_single_track = TempDir::new_in(test_directory).unwrap();
    let rom_directory_multiple_discs = TempDir::new_in(test_directory).unwrap();
    set_directory(
        &mut connection,
        "ROM_DIRECTORY",
        &PathBuf::from(rom_directory_single_track.path()),
        Some(system_single_track.id),
    )
    .await;
    set_directory(
        &mut connection,
        "ROM_DIRECTORY",
        &PathBuf::from(rom_directory_multiple_discs.path()),
        Some(system_multiple_discs.id),
    )
    .await;

    let system_directory_single_track = get_system_directory(&mut connection, &system_single_track)
        .await
        .unwrap();
    let system_directory_multiple_discs =
        get_system_directory(&mut connection, &system_multiple_discs)
            .await
            .unwrap();

    // when
    let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Single Track).chd");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (Single Track).chd"),
        &romfile_path,
    )
    .await
    .unwrap();
    import_chd(
        &mut connection,
        &progress_bar,
        &Some(&system_single_track),
        &HashSet::new(),
        CommonRomfile::from_path(&romfile_path).unwrap(),
        true,
        false,
        UnattendedMode::Skip,
    )
    .await
    .unwrap();

    let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Disc 1).iso");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (Disc 1).iso"),
        &romfile_path,
    )
    .await
    .unwrap();
    import_other(
        &mut connection,
        &progress_bar,
        &Some(&system_multiple_discs),
        &None,
        &HashSet::new(),
        CommonRomfile::from_path(&romfile_path).unwrap(),
        true,
        false,
        UnattendedMode::Skip,
    )
    .await
    .unwrap();

    // then
    let roms_single_track =
        find_roms_with_romfile_by_system_id(&mut connection, system_single_track.id).await;
    assert_eq!(roms_single_track.len(), 1);
    let romfile_single_track = find_romfile_by_id(
        &mut connection,
        roms_single_track.first().unwrap().romfile_id.unwrap(),
    )
    .await;
    assert_eq!(
        romfile_single_track.path,
        system_directory_single_track
            .join("Test Game (USA, Europe) (CUE BIN).chd")
            .strip_prefix(rom_directory_single_track.path())
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(
        rom_directory_single_track
            .path()
            .join(&romfile_single_track.path)
            .is_file()
    );
    assert!(
        !global_rom_directory
            .path()
            .join(&romfile_single_track.path)
            .is_file()
    );

    let roms_multiple_discs =
        find_roms_with_romfile_by_system_id(&mut connection, system_multiple_discs.id).await;
    assert_eq!(roms_multiple_discs.len(), 1);
    let romfile_multiple_discs = find_romfile_by_id(
        &mut connection,
        roms_multiple_discs.first().unwrap().romfile_id.unwrap(),
    )
    .await;
    assert_eq!(
        romfile_multiple_discs.path,
        system_directory_multiple_discs
            .join("Test Game (USA, Europe) (Disc 1).iso")
            .strip_prefix(rom_directory_multiple_discs.path())
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(
        rom_directory_multiple_discs
            .path()
            .join(&romfile_multiple_discs.path)
            .is_file()
    );
    assert!(
        !global_rom_directory
            .path()
            .join(&romfile_multiple_discs.path)
            .is_file()
    );
}
