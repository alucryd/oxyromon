use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20240704) (IRD).dat").await;

    let ird_path = env.fixture_source("Test Game (USA).ird");

    let game = find_games(&mut env.connection).await.remove(0);

    let (ird, data) = parse_ird(&ird_path).await.unwrap();

    let parent_roms = find_roms_by_game_id_no_parents(&mut env.connection, game.id).await;

    // when
    import_ird(
        &mut env.connection,
        &env.progress_bar,
        &game,
        &ird,
        &data,
        parent_roms.first(),
    )
    .await
    .unwrap();

    // then
    let roms = find_roms(&mut env.connection).await;
    assert_eq!(roms.len(), 208);
}
