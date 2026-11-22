use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20230105) (Multiple Discs).dat").await;

    for i in 1..=2 {
        let romfile_name = format!("Test Game (USA, Europe) (Disc {}).iso", i);
        let romfile_path = env.fixture(&romfile_name).await;
        env.import_roms(&[&romfile_path]).await;
    }

    env.run(&["generate-playlists", "-a"]).await.unwrap();

    let system = env.system().await;
    let games = find_games_by_system_id(&mut env.connection, system.id).await;
    let playlist_id = games[0].playlist_id.unwrap();
    assert_eq!(games[1].playlist_id, Some(playlist_id));

    // when one disc is purged
    purge_games(&mut env.connection, &env.progress_bar, &games[..1])
        .await
        .unwrap();

    // then the playlist stays, still listed by both discs
    let playlist = find_romfile_by_id(&mut env.connection, playlist_id).await;
    assert!(!playlist.path.contains("Trash"));
    let games = find_games_by_system_id(&mut env.connection, system.id).await;
    assert!(
        games
            .iter()
            .all(|game| game.playlist_id == Some(playlist_id))
    );

    // when the other disc is purged
    purge_games(&mut env.connection, &env.progress_bar, &games[1..])
        .await
        .unwrap();

    // then the playlist joins the discs in the Trash, and no game lists it
    let playlist = find_romfile_by_id(&mut env.connection, playlist_id).await;
    assert!(playlist.path.contains("Trash"));
    assert!(
        playlist
            .as_common(&mut env.connection)
            .await
            .unwrap()
            .path
            .is_file()
    );
    let games = find_games_by_system_id(&mut env.connection, system.id).await;
    assert!(games.iter().all(|game| game.playlist_id.is_none()));
    assert_eq!(find_romfiles_in_trash(&mut env.connection).await.len(), 3);
}
