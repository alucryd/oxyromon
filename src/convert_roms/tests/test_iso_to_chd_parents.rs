use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20230105) (Multiple Discs).dat").await;

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    for i in 1..=2 {
        let romfile_name = format!("Test Game (USA, Europe) (Disc {}).iso", i);
        let romfile_path = env.tmp_directory.join(&romfile_name);
        fs::copy(env.fixture_source(&romfile_name), &romfile_path)
            .await
            .unwrap();
        env.import_roms(&[&romfile_path]).await;
    }

    env.run(&["generate-playlists", "-a"]).await.unwrap();

    let games = find_full_games_by_system_id(&mut env.connection, system.id).await;
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    for game in &games {
        let roms = find_roms_with_romfile_by_game_ids(&mut env.connection, &[game.id]).await;
        for rom in &roms {
            let romfile = find_romfile_by_id(&mut env.connection, rom.romfile_id.unwrap()).await;
            romfiles_by_id.insert(romfile.id, romfile);
        }
        roms_by_game_id.insert(game.id, roms);
    }
    let games_by_id: HashMap<i64, Game> = games.into_iter().map(|game| (game.id, game)).collect();

    // when
    to_chd(
        &mut env.connection,
        &env.progress_bar,
        games_by_id,
        roms_by_game_id,
        romfiles_by_id,
        false,
        true,
        true,
        &[],
        &None,
        &[],
        &None,
        &[],
        &None,
        &[],
        &None,
        true,
        false,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 2);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 3);

    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe) (Disc 1).iso");

    let romfile = romfiles.first().unwrap();
    let parent_romfile_id = romfile.id;
    assert_eq!(
        romfile.path,
        env.stored_path(&system_directory.join("Test Game (USA, Europe) (Disc 1).chd")),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));
    assert!(romfile.parent_id.is_none());

    let rom = roms.get(1).unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe) (Disc 2).iso");

    let romfile = romfiles.get(1).unwrap();
    assert_eq!(
        romfile.path,
        env.stored_path(&system_directory.join("Test Game (USA, Europe) (Disc 2).chd")),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));
    assert_eq!(romfile.parent_id, Some(parent_romfile_id));
}
