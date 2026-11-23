use crate::import_roms::UnattendedMode;
use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let dat_path = env.fixture_source("Test System (20200721) (MAME).dat");
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

    // an arcade game that is not an archive lives in its own directory
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(
        romfiles[0]
            .as_common(&mut env.connection)
            .await
            .unwrap()
            .path,
        system_directory
            .join("Test Game (USA, Europe)")
            .join("Test Game (USA, Europe).rom")
    );

    let dat_path = env.fixture_source("Test System (20200722) (MAME) (Renamed Game).dat");
    let (datfile_xml, detector_xml) = parse_dat(&env.progress_bar, &dat_path, false)
        .await
        .unwrap();

    // when
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

    // then
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    assert_eq!(
        romfiles[0]
            .as_common(&mut env.connection)
            .await
            .unwrap()
            .path,
        system_directory
            .join("Renamed Game (USA, Europe)")
            .join("Renamed Game (USA, Europe).rom")
    );
    assert!(!system_directory.join("Test Game (USA, Europe)").exists());
    assert!(system_directory.is_dir());
}
