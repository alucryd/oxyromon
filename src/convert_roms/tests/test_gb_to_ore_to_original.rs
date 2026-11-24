use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261114) (Game Boy).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe).gb").await;

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    env.import_roms(&[&romfile_path]).await;

    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    let romfile = find_romfile_by_id(&mut env.connection, roms[0].romfile_id.unwrap()).await;
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    romfiles_by_id.insert(romfile.id, romfile);

    // when
    to_ore(
        &mut env.connection,
        &env.progress_bar,
        roms_by_game_id,
        romfiles_by_id,
        false,
        true,
        true,
        &OreCompressionAlgorithm::Auto,
        &OreProfile::Stream,
        None,
        None,
    )
    .await
    .unwrap();

    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    let romfile = find_romfile_by_id(&mut env.connection, roms[0].romfile_id.unwrap()).await;
    assert!(romfile.path.ends_with(".ore"));
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    romfiles_by_id.insert(romfile.id, romfile);

    to_original(
        &mut env.connection,
        &env.progress_bar,
        &system,
        roms_by_game_id,
        romfiles_by_id,
        true,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 1);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);

    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe).gb");

    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        env.stored_path(&system_directory.join("Test Game (USA, Europe).gb")),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));
    // Back as it was, under its own name
    assert_eq!(
        std::fs::read(env.rom_directory.join(&romfile.path)).unwrap(),
        std::fs::read(env.fixture_source("Test Game (USA, Europe).gb")).unwrap()
    );
}
