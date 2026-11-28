use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given a legacy lossless WUA in the library, its update partition in the
    // legacy store beside it
    let mut env = TestEnv::with_dat("Test System (20261025) (WUD).dat").await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    fs::create_dir_all(&system_directory).await.unwrap();

    let wua_path = system_directory.join("Test Game (USA).wua");
    let keys = wux_rs::load_keys(&env.fixture_source("Test Game (USA).keys.txt")).unwrap();
    wux_rs::to_wua(
        &[&env.fixture_source("Test Game (USA).wux")],
        &wua_path,
        &keys,
        Some(&system_directory.join(wux_rs::STORE_DIR)),
        &mut |_| {},
    )
    .unwrap();

    env.import_roms(&[&wua_path]).await;

    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    let romfile = find_romfile_by_id(&mut env.connection, roms[0].romfile_id.unwrap()).await;
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    romfiles_by_id.insert(romfile.id, romfile);

    // when
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

    // then the disc, named as Redump names it
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        env.stored_path(&system_directory.join("Test Game (USA).iso")),
    );
    assert_eq!(
        fs::read(env.rom_directory.join(&romfile.path))
            .await
            .unwrap(),
        fs::read(env.fixture_source("Test Game (USA).wud"))
            .await
            .unwrap()
    );
}
