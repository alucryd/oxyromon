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

    let system = env.system().await;

    // when
    env.run(&[
        "purge-games",
        "-s",
        &system.name,
        "-g",
        "Test Game (USA, Europe)",
    ])
    .await
    .unwrap();

    // a later sort must not bring the file back
    env.run(&["sort-roms", "-a", "-y"]).await.unwrap();

    // then
    let games = find_games_by_system_id(&mut env.connection, system.id).await;
    let purged = games
        .iter()
        .find(|game| game.name == "Test Game (USA, Europe)")
        .unwrap();
    assert_eq!(purged.completion, 0);
    assert!(
        find_roms_with_romfile_by_game_id(&mut env.connection, purged.id)
            .await
            .is_empty()
    );
    let kept = games
        .iter()
        .find(|game| game.name == "Test Game (Japan)")
        .unwrap();
    assert_eq!(kept.completion, 2);

    let trash_directory = get_trash_directory(&mut env.connection, Some(&system))
        .await
        .unwrap();
    assert!(
        trash_directory
            .join("Test Game (USA, Europe).rom")
            .is_file()
    );

    let romfiles = find_romfiles_in_trash(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    assert!(
        find_roms_by_romfile_id(&mut env.connection, romfiles[0].id)
            .await
            .is_empty()
    );
}
