use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe) (Full).7z").await;

    let system = env.system().await;

    env.import_roms(&[&romfile_path]).await;

    let games = find_full_games_by_system_id(&mut env.connection, system.id).await;
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    let games_by_id: HashMap<i64, Game> = games.into_iter().map(|game| (game.id, game)).collect();
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    for rom in &roms {
        let romfile = find_romfile_by_id(&mut env.connection, rom.romfile_id.unwrap()).await;
        romfiles_by_id.insert(romfile.id, romfile);
    }
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);

    let destination_directory = env.tmp_directory.join("destination");
    create_directory(&env.progress_bar, &destination_directory, true)
        .await
        .unwrap();

    // when
    to_chd(
        &mut env.connection,
        &env.progress_bar,
        &destination_directory,
        games_by_id,
        roms_by_game_id,
        romfiles_by_id,
        &[],
        &None,
        &[],
        &None,
    )
    .await
    .unwrap();

    // then
    assert!(
        destination_directory
            .join("Test Game (USA, Europe) (CUE BIN).chd")
            .is_file()
    );
}
