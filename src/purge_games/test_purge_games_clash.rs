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

    // a file of the same name already waits in the Trash
    let system = env.system().await;
    let trash_directory = get_trash_directory(&mut env.connection, Some(&system))
        .await
        .unwrap();
    fs::create_dir_all(&trash_directory).await.unwrap();
    fs::write(trash_directory.join(romfile_name), b"already trashed")
        .await
        .unwrap();
    let game = find_games_by_system_id(&mut env.connection, system.id)
        .await
        .into_iter()
        .find(|game| game.name == "Test Game (USA, Europe)")
        .unwrap();

    // when
    purge_games(
        &mut env.connection,
        &env.progress_bar,
        std::slice::from_ref(&game),
    )
    .await
    .unwrap();

    // then neither file is touched, and the game keeps its file
    assert_eq!(
        fs::read(trash_directory.join(romfile_name)).await.unwrap(),
        b"already trashed"
    );
    let roms = find_roms_with_romfile_by_game_id(&mut env.connection, game.id).await;
    assert_eq!(roms.len(), 1);
    let romfile = find_romfile_by_id(&mut env.connection, roms[0].romfile_id.unwrap()).await;
    assert!(!romfile.path.contains("Trash"));
    assert!(
        romfile
            .as_common(&mut env.connection)
            .await
            .unwrap()
            .path
            .is_file()
    );
}
