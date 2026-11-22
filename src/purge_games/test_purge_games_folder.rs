use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261025) (CDN).dat").await;

    let game_name = "Test Game (USA) (Update)";
    let nus_directory = env.tmp_directory.join(game_name);
    std::fs::create_dir(&nus_directory).unwrap();
    for entry in std::fs::read_dir(env.fixture_source(game_name)).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, nus_directory.join(path.file_name().unwrap())).unwrap();
    }
    let env_tmp_directory_1 = env.tmp_directory.clone();
    env.import_roms(&[&env_tmp_directory_1]).await;

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();
    let game = find_games_by_system_id(&mut env.connection, system.id)
        .await
        .remove(0);
    assert_eq!(game.completion, 2);

    // when
    purge_games(
        &mut env.connection,
        &env.progress_bar,
        std::slice::from_ref(&game),
    )
    .await
    .unwrap();
    env.run(&["sort-roms", "-a", "-y"]).await.unwrap();

    // then the title's folder moved whole into the Trash, and its old place is gone
    let trash_directory = get_trash_directory(&mut env.connection, Some(&system))
        .await
        .unwrap();
    for name in [
        "00000010",
        "00000011",
        "00000012",
        "00000012.h3",
        "cetk.48",
        "tmd.48",
    ] {
        let path = trash_directory.join(game_name).join(name);
        assert!(path.is_file(), "{}", path.display());
    }
    assert!(!system_directory.join(game_name).exists());
    assert_eq!(find_romfiles_in_trash(&mut env.connection).await.len(), 6);
    let game = find_game_by_id(&mut env.connection, game.id).await;
    assert_eq!(game.completion, 0);
}
