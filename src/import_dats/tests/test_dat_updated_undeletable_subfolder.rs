use super::*;
use crate::config::*;
use crate::import_roms::UnattendedMode;
use crate::test_support::TestEnv;
use std::os::unix::fs::PermissionsExt;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    set_string(&mut env.connection, "REGIONS_ALL_SUBFOLDERS", "alpha", None).await;

    let dat_path = env.fixture_source("Test System (20200721).dat");
    let (datfile_xml, detector_xml) = parse_dat(&env.progress_bar, &dat_path, false)
        .await
        .unwrap();
    import_dat(
        &mut env.connection,
        &env.progress_bar,
        &datfile_xml,
        &detector_xml,
        None,
        None,
        false,
        false,
    )
    .await
    .unwrap();

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();
    let romfile_path = env.fixture("Test Game (USA, Europe).rom").await;
    import_rom(
        &mut env.connection,
        &env.progress_bar,
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

    // the file can still move into an existing Trash, but the emptied
    // subfolder cannot be deleted from a read-only system directory
    // (root ignores the permission, and then simply deletes it)
    fs::create_dir_all(system_directory.join("Trash"))
        .await
        .unwrap();
    std::fs::set_permissions(&system_directory, std::fs::Permissions::from_mode(0o555)).unwrap();

    let dat_path = env.fixture_source("Test System (20200723) (Changed Hashes).dat");
    let (datfile_xml, detector_xml) = parse_dat(&env.progress_bar, &dat_path, false)
        .await
        .unwrap();

    // when
    let result = import_dat(
        &mut env.connection,
        &env.progress_bar,
        &datfile_xml,
        &detector_xml,
        None,
        None,
        false,
        false,
    )
    .await;
    std::fs::set_permissions(&system_directory, std::fs::Permissions::from_mode(0o755)).unwrap();

    // then
    // the update still completes, and the database follows the file
    result.unwrap();
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    let romfile_path = romfiles[0]
        .as_common(&mut env.connection)
        .await
        .unwrap()
        .path;
    assert_eq!(
        romfile_path,
        system_directory
            .join("Trash")
            .join("Test Game (USA, Europe).rom")
    );
    assert!(romfile_path.is_file());
}
