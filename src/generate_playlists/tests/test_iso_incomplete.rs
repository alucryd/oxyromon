use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20230105) (Multiple Discs).dat").await;

    let system = env.system().await;

    let romfile_name = "Test Game (USA, Europe) (Disc 1).iso";
    let romfile_path = env.fixture(romfile_name).await;
    env.import_roms(&[&romfile_path]).await;

    // when
    process_system(&mut env.connection, &env.progress_bar, &system)
        .await
        .unwrap();

    // then
    let games = find_full_games_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(games.len(), 1);

    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 1);

    let playlist_id = games.first().unwrap().playlist_id;
    assert!(playlist_id.is_none());
}
