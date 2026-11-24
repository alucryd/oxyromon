use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261025) (CDN).dat").await;

    let update_directory = env.fixture("Test Game (USA) (Update)").await;
    env.run(&["import-roms", update_directory.to_str().unwrap()])
        .await
        .unwrap();

    let system = env.system().await;
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

    // then its files, named alike in every title, are in a folder of its own
    let game_directory = destination_directory.join("Test Game (USA) (Update)");
    for name in [
        "00000010",
        "00000011",
        "00000012",
        "00000012.h3",
        "cetk.48",
        "tmd.48",
    ] {
        assert!(game_directory.join(name).is_file(), "{name}");
    }
}
