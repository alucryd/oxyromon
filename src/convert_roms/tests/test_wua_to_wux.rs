use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261025) (WUD).dat").await;

    let romfile_path = env.fixture("Test Game (USA).wux").await;

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    env.import_roms(&[&romfile_path]).await;

    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    let romfile = find_romfile_by_id(&mut env.connection, roms[0].romfile_id.unwrap()).await;
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    romfiles_by_id.insert(romfile.id, romfile);

    to_wua(
        &mut env.connection,
        &env.progress_bar,
        &system,
        roms_by_game_id,
        romfiles_by_id,
        false,
        true,
    )
    .await
    .unwrap();

    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    let romfile = find_romfile_by_id(&mut env.connection, roms[0].romfile_id.unwrap()).await;
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    romfiles_by_id.insert(romfile.id, romfile);

    // when
    to_wux(
        &mut env.connection,
        &env.progress_bar,
        roms_by_game_id,
        romfiles_by_id,
        false,
        true,
    )
    .await
    .unwrap();

    // then the WUX is the one the WUA was made from, byte for byte
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        env.stored_path(&system_directory.join("Test Game (USA).wux")),
    );
    assert_eq!(
        fs::read(env.rom_directory.join(&romfile.path))
            .await
            .unwrap(),
        fs::read(env.fixture_source("Test Game (USA).wux"))
            .await
            .unwrap()
    );
    assert!(!system_directory.join("Test Game (USA).wua").exists());
}
