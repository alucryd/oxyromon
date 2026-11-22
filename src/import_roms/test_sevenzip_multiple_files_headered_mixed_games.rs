use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20210402) (Headered).dat").await;

    let romfile_path = env
        .fixture("Test Game (USA, Europe) (Mixed) (Headered).7z")
        .await;

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();
    let header = find_header_by_system_id(&mut env.connection, system.id).await;

    // when
    import_archive(
        &mut env.connection,
        &env.progress_bar,
        &Some(&system),
        &header,
        &HashSet::new(),
        CommonRomfile::from_path(&romfile_path).unwrap(),
        romfile_path.extension().unwrap().to_str().unwrap(),
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
    assert_eq!(game.name, "Test Game (USA, Europe)");
    assert_eq!(game.system_id, system.id);

    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe).rom")
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());

    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe).rom");
    assert_eq!(rom.game_id, game.id);
    assert_eq!(rom.romfile_id, Some(romfile.id));
}
