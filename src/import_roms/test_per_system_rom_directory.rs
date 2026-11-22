use super::*;
use crate::test_support::TestEnv;
use std::collections::HashSet;
use tempfile::TempDir;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    env.import_dats(&[
        "Test System (20240229) (Single Track).dat",
        "Test System (20230105) (Multiple Discs).dat",
    ])
    .await;

    let mut systems = find_systems(&mut env.connection).await;
    assert_eq!(systems.len(), 2);
    // find_systems returns systems ordered by name:
    // "Test System" < "Test System (Multiple Discs)"
    let system_single_track = systems.remove(0);
    let system_multiple_discs = systems.remove(0);

    let rom_directory_single_track = TempDir::new().unwrap();
    let rom_directory_multiple_discs = TempDir::new().unwrap();
    set_directory(
        &mut env.connection,
        "ROM_DIRECTORY",
        &rom_directory_single_track.path().to_path_buf(),
        Some(system_single_track.id),
    )
    .await;
    set_directory(
        &mut env.connection,
        "ROM_DIRECTORY",
        &rom_directory_multiple_discs.path().to_path_buf(),
        Some(system_multiple_discs.id),
    )
    .await;

    let system_directory_single_track = env.system_directory(&system_single_track).await;
    let system_directory_multiple_discs = env.system_directory(&system_multiple_discs).await;

    // when
    let romfile_path = env
        .fixture("Test Game (USA, Europe) (Single Track).chd")
        .await;
    import_chd(
        &mut env.connection,
        &env.progress_bar,
        &Some(&system_single_track),
        &HashSet::new(),
        CommonRomfile::from_path(&romfile_path).unwrap(),
        true,
        false,
        UnattendedMode::Skip,
    )
    .await
    .unwrap();

    let romfile_path = env.fixture("Test Game (USA, Europe) (Disc 1).iso").await;
    import_other(
        &mut env.connection,
        &env.progress_bar,
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
        find_roms_with_romfile_by_system_id(&mut env.connection, system_single_track.id).await;
    assert_eq!(roms_single_track.len(), 1);
    let romfile_single_track = find_romfile_by_id(
        &mut env.connection,
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
    assert!(!env.rom_directory.join(&romfile_single_track.path).is_file());

    let roms_multiple_discs =
        find_roms_with_romfile_by_system_id(&mut env.connection, system_multiple_discs.id).await;
    assert_eq!(roms_multiple_discs.len(), 1);
    let romfile_multiple_discs = find_romfile_by_id(
        &mut env.connection,
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
        !env.rom_directory
            .join(&romfile_multiple_discs.path)
            .is_file()
    );
}
