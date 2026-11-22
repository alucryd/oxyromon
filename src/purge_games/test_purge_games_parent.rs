use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    for romfile_name in ["Test Game (USA, Europe).rom", "Test Game (Japan).rom"] {
        let romfile_path = env.fixture(romfile_name).await;
        env.import_roms(&[&romfile_path]).await;
    }

    // the Japanese file depends on the American one, as a child CHD on its parent
    let system = env.system().await;
    let games = find_games_by_system_id(&mut env.connection, system.id).await;
    let parent_game = games
        .iter()
        .find(|game| game.name == "Test Game (USA, Europe)")
        .unwrap();
    let child_game = games
        .iter()
        .find(|game| game.name == "Test Game (Japan)")
        .unwrap();
    let parent_romfile_id = find_roms_with_romfile_by_game_id(&mut env.connection, parent_game.id)
        .await
        .remove(0)
        .romfile_id
        .unwrap();
    let child_romfile_id = find_roms_with_romfile_by_game_id(&mut env.connection, child_game.id)
        .await
        .remove(0)
        .romfile_id
        .unwrap();
    update_romfile_parent(
        &mut env.connection,
        child_romfile_id,
        Some(parent_romfile_id),
    )
    .await;

    // when the parent's game is purged alone
    purge_games(
        &mut env.connection,
        &env.progress_bar,
        std::slice::from_ref(parent_game),
    )
    .await
    .unwrap();

    // then its file stays for the child
    let parent_romfile = find_romfile_by_id(&mut env.connection, parent_romfile_id).await;
    assert!(!parent_romfile.path.contains("Trash"));
    assert_eq!(
        find_roms_by_romfile_id(&mut env.connection, parent_romfile_id)
            .await
            .len(),
        1
    );

    // when both games are purged
    let both_games: Vec<Game> = find_games_by_system_id(&mut env.connection, system.id)
        .await
        .into_iter()
        .filter(|game| game.id == parent_game.id || game.id == child_game.id)
        .collect();
    purge_games(&mut env.connection, &env.progress_bar, &both_games)
        .await
        .unwrap();

    // then both files go
    for romfile_id in [parent_romfile_id, child_romfile_id] {
        let romfile = find_romfile_by_id(&mut env.connection, romfile_id).await;
        assert!(romfile.path.contains("Trash"), "{}", romfile.path);
        assert!(
            find_roms_by_romfile_id(&mut env.connection, romfile_id)
                .await
                .is_empty()
        );
    }
}
