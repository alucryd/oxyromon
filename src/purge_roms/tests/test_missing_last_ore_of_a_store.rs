use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given a store whose only game went missing
    let mut env = TestEnv::with_dat("Test System (20261114) (Game Boy).dat").await;
    let romfile_path = env.fixture("Test Game (USA, Europe).gb.7z").await;
    env.import_roms(&[&romfile_path]).await;
    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();
    let store = system_directory.join(ore_rs::STORE_DIR);
    set_bool(&mut env.connection, "ORE_STORE", true, Some(system.id)).await;
    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();
    assert!(store.is_dir());
    std::fs::remove_file(system_directory.join("Test Game (USA, Europe).ore")).unwrap();

    // when
    purge_missing_romfiles(&mut env.connection, &env.progress_bar)
        .await
        .unwrap();

    // then the store goes with its last game
    assert!(find_romfiles(&mut env.connection).await.is_empty());
    assert!(!store.exists());
}
