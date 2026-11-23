use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe).rom").await;
    env.import_roms(&[&romfile_path]).await;

    // another game's ROM stored in the same file, as in a merged set
    let system = env.system().await;
    let games = find_games_by_system_id(&mut env.connection, system.id).await;
    let game = games
        .iter()
        .find(|game| game.name == "Test Game (USA, Europe)")
        .unwrap();
    let other_game = games
        .iter()
        .find(|game| game.name == "Test Game (Japan)")
        .unwrap();
    let romfile_id = find_roms_with_romfile_by_game_id(&mut env.connection, game.id)
        .await
        .remove(0)
        .romfile_id;
    let other_rom = find_roms_by_game_id_no_parents(&mut env.connection, other_game.id)
        .await
        .remove(0);
    update_rom_romfile(&mut env.connection, other_rom.id, romfile_id).await;

    // when
    purge_games(
        &mut env.connection,
        &env.progress_bar,
        std::slice::from_ref(game),
    )
    .await
    .unwrap();

    // then
    let romfile = find_romfile_by_id(&mut env.connection, romfile_id.unwrap()).await;
    assert!(!romfile.path.contains("Trash"));
    assert_eq!(
        find_roms_by_romfile_id(&mut env.connection, romfile.id)
            .await
            .len(),
        2
    );
}
