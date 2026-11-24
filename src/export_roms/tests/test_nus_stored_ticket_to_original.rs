use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261025) (CDN No Ticket).dat").await;

    let update_directory = env.tmp_directory.join("Test Game (USA) (Update)");
    std::fs::create_dir(&update_directory).unwrap();
    // with the ticket a downloader made, which the DAT does not list
    for entry in std::fs::read_dir(env.fixture_source("Test Game (USA) (Update)")).unwrap() {
        let path = entry.unwrap().path();
        let name = match path.file_name().unwrap().to_str().unwrap() {
            "cetk.48" => "title.tik",
            name => name,
        };
        std::fs::copy(&path, update_directory.join(name)).unwrap();
    }
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

    // then the stored ticket goes with the title, as WUP Installer needs it
    let game_directory = destination_directory.join("Test Game (USA) (Update)");
    assert_eq!(
        std::fs::read(game_directory.join("title.tik")).unwrap(),
        std::fs::read(
            env.fixture_source("Test Game (USA) (Update)")
                .join("cetk.48")
        )
        .unwrap()
    );
    for name in ["00000010", "00000011", "00000012", "00000012.h3", "tmd.48"] {
        assert!(game_directory.join(name).is_file(), "{name}");
    }
}
