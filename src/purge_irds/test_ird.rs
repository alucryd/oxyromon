use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20240704) (IRD).dat").await;

    env.run(&["import-irds", "tests/Test Game (USA).ird"])
        .await
        .unwrap();

    let game = find_games(&mut env.connection).await.remove(0);

    let roms_before = find_roms(&mut env.connection).await;
    assert_eq!(roms_before.len(), 208);

    // Refetch game to get updated jbfolder status
    let game = find_game_by_id(&mut env.connection, game.id).await;
    assert!(game.jbfolder);

    // when
    purge_ird(&mut env.connection, &env.progress_bar, &game)
        .await
        .unwrap();

    // then
    let roms_after = find_roms(&mut env.connection).await;
    assert_eq!(roms_after.len(), 1); // Only the parent rom should remain

    let updated_game = find_game_by_id(&mut env.connection, game.id).await;
    assert!(!updated_game.jbfolder);
}
