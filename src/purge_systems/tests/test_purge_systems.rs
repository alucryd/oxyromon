use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let romfile_names = vec![
        "Test Game (Asia).rom",
        "Test Game (Japan).rom",
        "Test Game (USA, Europe).rom",
        "Test Game (USA, Europe) (Beta).rom",
    ];
    for romfile_name in &romfile_names {
        let romfile_path = env.fixture(romfile_name).await;
        env.import_roms(&[&romfile_path]).await;
    }

    let systems = find_systems(&mut env.connection).await;

    // when
    purge_system(
        &mut env.connection,
        &env.progress_bar,
        systems.first().unwrap(),
    )
    .await
    .unwrap();

    // then
    let systems = find_systems(&mut env.connection).await;
    assert_eq!(systems.len(), 0);

    let games = find_games(&mut env.connection).await;
    assert_eq!(games.len(), 0);

    let roms = find_roms(&mut env.connection).await;
    assert_eq!(roms.len(), 0);

    let romfiles = find_romfiles(&mut env.connection).await;

    for romfile in romfiles {
        assert!(romfile.path.contains("Trash"))
    }
}
