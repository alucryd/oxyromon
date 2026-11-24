use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261025) (WUD).dat").await;

    let romfile_path = env.fixture("Test Game (USA).wux").await;

    let system = env.system().await;

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
    to_wux(
        &mut env.connection,
        &env.progress_bar,
        &destination_directory,
        roms_by_game_id,
        romfiles_by_id,
    )
    .await
    .unwrap();

    // then
    assert_eq!(
        fs::read(destination_directory.join("Test Game (USA).wux"))
            .await
            .unwrap(),
        fs::read(env.fixture_source("Test Game (USA).wux"))
            .await
            .unwrap(),
    );
}
