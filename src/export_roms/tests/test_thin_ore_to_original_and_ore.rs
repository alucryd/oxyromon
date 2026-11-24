use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261114) (Game Boy).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe).gb.7z").await;

    let system = env.system().await;

    env.import_roms(&[&romfile_path]).await;

    // a thin ORE, its chunks in the system's store
    set_bool(&mut env.connection, "ORE_STORE", true, Some(system.id)).await;
    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();

    let load = async |connection: &mut SqliteConnection| {
        let roms = find_roms_with_romfile_by_system_id(connection, system.id).await;
        let romfile = find_romfile_by_id(connection, roms[0].romfile_id.unwrap()).await;
        assert!(romfile.path.ends_with(".ore"));
        let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
        roms_by_game_id.insert(roms[0].game_id, roms);
        let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
        romfiles_by_id.insert(romfile.id, romfile);
        (roms_by_game_id, romfiles_by_id)
    };
    let rom = fs::read(env.fixture_source("Test Game (USA, Europe).gb"))
        .await
        .unwrap();

    // when exported as its original
    let original_directory = env.tmp_directory.join("original");
    create_directory(&env.progress_bar, &original_directory, true)
        .await
        .unwrap();
    let (roms_by_game_id, romfiles_by_id) = load(&mut env.connection).await;
    let games_by_id: HashMap<i64, Game> = find_games_by_system_id(&mut env.connection, system.id)
        .await
        .into_iter()
        .map(|game| (game.id, game))
        .collect();
    to_original(
        &mut env.connection,
        &env.progress_bar,
        &original_directory,
        &system,
        games_by_id,
        roms_by_game_id,
        romfiles_by_id,
    )
    .await
    .unwrap();

    // then the ROM comes out under its name, read through the store
    let exported = fs::read(original_directory.join("Test Game (USA, Europe).gb"))
        .await
        .unwrap();
    assert!(exported == rom);

    // when exported as an ORE
    let ore_directory = env.tmp_directory.join("ore");
    create_directory(&env.progress_bar, &ore_directory, true)
        .await
        .unwrap();
    let (roms_by_game_id, romfiles_by_id) = load(&mut env.connection).await;
    to_ore(
        &mut env.connection,
        &env.progress_bar,
        &ore_directory,
        roms_by_game_id,
        romfiles_by_id,
    )
    .await
    .unwrap();

    // then it is standalone, reading with no store beside it
    let ore = ore_directory.join("Test Game (USA, Europe).ore");
    let mut reader = ore_rs::Reader::open(&ore).unwrap();
    assert!(reader.store_uuid().is_none());
    let mut bytes = vec![0; rom.len()];
    reader.read_at(0, &mut bytes).unwrap();
    assert!(bytes == rom);
}
