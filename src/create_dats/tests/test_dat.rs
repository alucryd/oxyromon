use crate::database::*;
use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;

    let system_directory = env.tmp_directory.join("Test System");
    tokio::fs::create_dir(&system_directory).await.unwrap();
    env.fixture_into(&system_directory, "Test Game (USA, Europe).rom")
        .await;

    let dat_directory = env.tmp_directory.join("DAT");
    let dat_path = dat_directory.join("Test System (42).dat");

    // when
    create_dat(
        &mut env.connection,
        &env.progress_bar,
        &system_directory,
        Some(&dat_directory),
        None,
        None,
        Some(&String::from("42")),
        None,
        None,
    )
    .await
    .unwrap();
    env.run(&["import-dats", dat_path.to_str().unwrap()])
        .await
        .unwrap();

    // then
    let systems = find_systems(&mut env.connection).await;
    assert_eq!(systems.len(), 1);
    let system = systems.first().unwrap();
    assert_eq!(system.name, "Test System");
    let games = find_games_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(games.len(), 1);
    let game = games.first().unwrap();
    assert_eq!(game.name, "Test Game (USA, Europe)");
    let roms = find_roms_without_romfile_by_game_ids(&mut env.connection, &[game.id]).await;
    assert_eq!(roms.len(), 1);
    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe).rom");
    assert_eq!(rom.game_id, game.id);
}
