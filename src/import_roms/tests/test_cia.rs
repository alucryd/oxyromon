use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20231024) (CIA).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe).cia").await;

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();

    // when
    import_cia(
        &mut env.connection,
        &env.progress_bar,
        &Some(&system),
        &HashSet::new(),
        CommonRomfile::from_path(&romfile_path).unwrap(),
        true,
        false,
        UnattendedMode::Skip,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 2);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
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

    let rom_0 = roms.first().unwrap();
    assert_eq!(rom_0.name, "00000000");
    assert_eq!(rom_0.game_id, game.id);

    let rom_1 = roms.get(1).unwrap();
    assert_eq!(rom_1.name, "tmd.2145");
    assert_eq!(rom_1.game_id, game.id);

    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe).cia")
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
    assert_eq!(rom_0.romfile_id, Some(romfile.id));
    assert_eq!(rom_1.romfile_id, Some(romfile.id));
}
