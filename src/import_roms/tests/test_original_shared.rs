use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    env.import_dats(&["Test System (20200721).dat"]).await;

    env.import_dats(&["Test System (20251214) (Shared Roms).dat"])
        .await;

    let romfile_paths = vec![
        env.tmp_directory.join("Test Game (USA, Europe) 1.rom"),
        env.tmp_directory.join("Test Game (USA, Europe) 2.rom"),
    ];
    for romfile_path in &romfile_paths {
        fs::copy(
            env.fixture_source("Test Game (USA, Europe).rom"),
            &romfile_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();
    }

    let systems = find_systems(&mut env.connection).await;
    let mut system_directories = vec![];
    for system in &systems {
        let system_directory = get_system_directory(&mut env.connection, system)
            .await
            .unwrap();
        system_directories.push(system_directory);
    }

    // when
    env.run(&[
        "import-roms",
        "-u",
        "first",
        romfile_paths.first().unwrap().to_str().unwrap(),
        romfile_paths.get(1).unwrap().to_str().unwrap(),
    ])
    .await
    .unwrap();

    // then
    for (i, system) in systems.iter().enumerate() {
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

        let rom = roms.first().unwrap();
        assert_eq!(rom.name, "Test Game (USA, Europe).rom");
        assert_eq!(rom.game_id, game.id);

        let romfile = romfiles.first().unwrap();
        assert_eq!(
            romfile.path,
            system_directories
                .get(i)
                .unwrap()
                .join("Test Game (USA, Europe).rom")
                .strip_prefix(&env.rom_directory)
                .unwrap()
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(env.rom_directory.join(&romfile.path).is_file());
        assert_eq!(rom.romfile_id, Some(romfile.id));
    }
}
