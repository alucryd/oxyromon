use super::super::import_roms::UnattendedMode;
use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
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

    let romfile_names = vec!["Test Game (USA, Europe) (Full).7z"];
    for romfile_name in romfile_names {
        let romfile_path = env.fixture(romfile_name).await;
        import_rom(
            &mut env.connection,
            &env.progress_bar,
            &Some(&system),
            &None,
            &romfile_path,
            true,
            false,
            true,
            UnattendedMode::Skip,
            false,
        )
        .await
        .unwrap();
    }

    let dat_path = env.fixture_source("Test System (20240626) (Multiple Orphan Roms Mismatch).dat");
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
    let systems = find_systems(&mut env.connection).await;
    assert_eq!(systems.len(), 1);

    let system = systems.first().unwrap();
    assert_eq!(system.name, "Test System");

    let games = find_games(&mut env.connection).await;
    let roms = find_roms(&mut env.connection).await;
    let romfiles = find_romfiles(&mut env.connection).await;

    assert_eq!(games.len(), 1);
    assert_eq!(roms.len(), 3);
    assert_eq!(romfiles.len(), 1);

    for rom in roms {
        assert!(rom.romfile_id.is_none())
    }

    let romfile = romfiles.first().unwrap();
    assert!(romfile.path.contains("/Trash/"))
}
