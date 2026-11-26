use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given a game of two disks, an ORE each
    let mut env = TestEnv::with_dat("Commodore - Amiga (20261125).dat").await;
    let archive = env.fixture("Test Game (Europe).7z").await;
    env.import_roms(&[&archive]).await;
    let system = env.system().await;
    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();

    let games = find_full_games_by_system_id(&mut env.connection, system.id).await;
    let (roms_by_game_id, games_by_id, romfiles_by_id) =
        load_rom_data(&mut env.connection, games, false).await;

    let destination_directory = env.tmp_directory.join("destination");
    create_directory(&env.progress_bar, &destination_directory, true)
        .await
        .unwrap();

    // when
    to_original(
        &mut env.connection,
        &env.progress_bar,
        &destination_directory,
        &system,
        games_by_id,
        roms_by_game_id,
        romfiles_by_id,
    )
    .await
    .unwrap();

    // then both disks are exported, not just the first
    for disk in 1..=2 {
        let name = format!("Test Game (Europe) (Disk {disk}).ipf");
        assert_eq!(
            std::fs::read(destination_directory.join(&name)).unwrap(),
            std::fs::read(env.fixture_source(&name)).unwrap(),
            "{name}"
        );
    }
}
