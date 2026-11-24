use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261025) (WUD).dat").await;

    // the update comes from a system of its own, as No-Intro's CDN DAT is
    env.import_dats(&["Test System (20261025) (CDN).dat"]).await;
    let update_directory = env.fixture("Test Game (USA) (Update)").await;
    env.run(&["import-roms", update_directory.to_str().unwrap()])
        .await
        .unwrap();

    let romfile_path = env.fixture("Test Game (USA).wux").await;

    let system = find_systems(&mut env.connection)
        .await
        .into_iter()
        .find(|system| system.name == "Test System (WUD)")
        .unwrap();

    env.import_roms(&[&romfile_path]).await;

    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    let romfile = find_romfile_by_id(&mut env.connection, roms[0].romfile_id.unwrap()).await;
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    romfiles_by_id.insert(romfile.id, romfile);

    let destination_directory = env.tmp_directory.join("destination");
    create_directory(&env.progress_bar, &destination_directory, true)
        .await
        .unwrap();

    // when
    to_wua(
        &mut env.connection,
        &env.progress_bar,
        &destination_directory,
        roms_by_game_id,
        romfiles_by_id,
    )
    .await
    .unwrap();

    // then the WUA holds the game and its update, a folder each
    let wua = fs::read(destination_directory.join("Test Game (USA).wua"))
        .await
        .unwrap();
    let holds = |name: &[u8]| wua.windows(name.len()).any(|w| w == name);
    assert!(holds(b"0005000010101010_v32"));
    assert!(holds(b"0005000e10101010_v48"));
    // and what reverts each to a NUS folder
    assert!(holds(b".nus"));
    assert!(holds(b"residue"));
}
