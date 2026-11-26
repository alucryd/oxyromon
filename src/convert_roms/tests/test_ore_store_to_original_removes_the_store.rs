use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given a game in the system's store
    let mut env = TestEnv::with_dat("Test System (20261114) (Game Boy).dat").await;
    let romfile_path = env.fixture("Test Game (USA, Europe).gb.7z").await;
    env.import_roms(&[&romfile_path]).await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    let store = system_directory.join(ore_rs::STORE_DIR);
    set_bool(&mut env.connection, "ORE_STORE", true, Some(system.id)).await;
    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();
    assert!(store.is_dir());

    // when its last game leaves ORE
    env.run(&["convert-roms", "-f", "ORIGINAL", "-a"])
        .await
        .unwrap();

    // then the store goes with it, and a check has none to check
    assert!(!store.exists());
    assert!(
        system_directory
            .join("Test Game (USA, Europe).gb")
            .is_file()
    );
    env.run(&["check-roms", "-a"]).await.unwrap();
    assert!(!store.exists());
}
