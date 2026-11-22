use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20241230) (MAME Mixed).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe).rom").await;
    let chd_romfile_path = env.fixture("Test Game (USA, Europe) (RDSK).chd").await;

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();

    // when
    import_other(
        &mut env.connection,
        &env.progress_bar,
        &Some(&system),
        &None,
        &HashSet::new(),
        CommonRomfile::from_path(&romfile_path).unwrap(),
        true,
        false,
        UnattendedMode::Skip,
    )
    .await
    .unwrap();
    import_chd(
        &mut env.connection,
        &env.progress_bar,
        &Some(&system),
        &HashSet::new(),
        CommonRomfile::from_path(&chd_romfile_path).unwrap(),
        true,
        false,
        UnattendedMode::Skip,
    )
    .await
    .unwrap();

    // then
    let mut roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 2);
    let mut romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 2);
    let games = find_games_by_ids(
        &mut env.connection,
        roms.iter()
            .map(|rom| rom.game_id)
            .collect::<Vec<i64>>()
            .as_slice(),
    )
    .await;
    assert_eq!(games.len(), 1);

    let game = games.first().unwrap();
    assert_eq!(game.name, "Test Game (USA, Europe)");
    assert_eq!(game.system_id, system.id);

    let rom = roms.pop().unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe).rom");
    assert_eq!(rom.game_id, game.id);

    let romfile = romfiles.pop().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe)")
            .join("Test Game (USA, Europe).rom")
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));

    let rom = roms.pop().unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe)");
    assert_eq!(rom.game_id, game.id);

    let romfile = romfiles.pop().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe)")
            .join("Test Game (USA, Europe).chd")
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));
}
