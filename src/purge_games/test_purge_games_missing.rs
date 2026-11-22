use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let romfile_name = "Test Game (USA, Europe).rom";
    let romfile_path = env.fixture(romfile_name).await;
    env.import_roms(&[&romfile_path]).await;

    // the file was deleted by hand
    let system = env.system().await;
    let game = find_games_by_system_id(&mut env.connection, system.id)
        .await
        .into_iter()
        .find(|game| game.name == "Test Game (USA, Europe)")
        .unwrap();
    let romfile_id = find_roms_with_romfile_by_game_id(&mut env.connection, game.id)
        .await
        .remove(0)
        .romfile_id
        .unwrap();
    let romfile = find_romfile_by_id(&mut env.connection, romfile_id).await;
    fs::remove_file(romfile.as_common(&mut env.connection).await.unwrap().path)
        .await
        .unwrap();

    // when
    purge_games(
        &mut env.connection,
        &env.progress_bar,
        std::slice::from_ref(&game),
    )
    .await
    .unwrap();

    // then its record is gone, and the game is missing
    assert!(find_romfiles(&mut env.connection).await.is_empty());
    assert!(
        find_roms_with_romfile_by_game_id(&mut env.connection, game.id)
            .await
            .is_empty()
    );
    let game = find_game_by_id(&mut env.connection, game.id).await;
    assert_eq!(game.completion, 0);
}
