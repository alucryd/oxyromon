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

    let romfile_names = vec![
        "Test Game (Asia).rom",
        "Test Game (Japan).rom",
        "Test Game (USA, Europe).rom",
    ];
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

    let dat_path = env.fixture_source("Test System (20210401).dat");
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

    assert_eq!(games.len(), 3);
    assert_eq!(roms.len(), 3);
    assert_eq!(romfiles.len(), 3);

    let game = games.first().unwrap();
    let rom = roms.first().unwrap();
    let romfile = romfiles.get(1).unwrap();

    assert_eq!(game.name, "Test Game (Asia)");
    assert_eq!(rom.name, "Test Game (Asia).rom");
    assert!(rom.romfile_id.is_none());
    assert!(romfile.path.contains("/Trash/"));

    let game = games.get(1).unwrap();
    let rom = roms.get(2).unwrap();
    let romfile = romfiles.get(2).unwrap();

    assert_eq!(game.name, "Test Game (USA, Europe)");
    assert_eq!(rom.name, "Updated Test Game (USA, Europe).rom");
    assert!(rom.romfile_id.is_some());
    assert_eq!(rom.romfile_id.unwrap(), romfile.id);

    let game = games.get(2).unwrap();
    let rom = roms.get(1).unwrap();
    let romfile = romfiles.first().unwrap();

    assert_eq!(game.name, "Updated Test Game (Japan)");
    assert_eq!(rom.name, "Test Game (Japan).rom");
    assert!(rom.romfile_id.is_some());
    assert_eq!(rom.romfile_id.unwrap(), romfile.id);
}
