use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20230618) (RVZ).dat").await;

    let romfile_path = env.tmp_directory.join("Test Game (USA).ore");
    ore_rs::compress(
        &env.fixture_source("Test Game (USA).iso"),
        &romfile_path,
        &ore_rs::Options::default(),
        &[],
        &mut |_| {},
    )
    .unwrap();

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();

    // when
    import_disc(
        &mut env.connection,
        &env.progress_bar,
        &Some(&system),
        &HashSet::new(),
        CommonRomfile::from_path(&romfile_path)
            .unwrap()
            .as_ore()
            .unwrap(),
        ORE_EXTENSION,
        true,
        false,
        UnattendedMode::Skip,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 1);
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
    assert_eq!(game.name, "Test Game (USA)");
    assert_eq!(game.system_id, system.id);

    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA).iso");
    assert_eq!(rom.game_id, game.id);

    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA).ore")
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));
}
