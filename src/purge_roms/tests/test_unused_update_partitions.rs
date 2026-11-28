use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given a legacy lossless WUA imported with its `.update-partitions` store
    let mut env = TestEnv::with_dat("Test System (20261025) (WUD).dat").await;
    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();
    fs::create_dir_all(&system_directory).await.unwrap();

    let wua = system_directory.join("Test Game (USA).wua");
    let store = system_directory.join(STORE_DIR);
    let keys = wux_rs::load_keys(&env.fixture_source("Test Game (USA).keys.txt")).unwrap();
    wux_rs::to_wua(
        &[&env.fixture_source("Test Game (USA).wux")],
        &wua,
        &keys,
        Some(&store),
        &mut |_| {},
    )
    .unwrap();
    env.import_roms(&[&wua]).await;
    let entry = store.join(&wux_rs::store_entries(&wua).unwrap()[0]);
    assert!(entry.is_file());

    // and converted out to ORE, leaving no WUA in the database
    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();
    assert!(!wua.exists());
    assert!(entry.is_file());

    // when purging (even without --foreign)
    env.run(&["purge-roms", "-t", "-y"]).await.unwrap();

    // then the dead update partition and empty `.update-partitions` directory are gone
    assert!(!entry.exists());
    assert!(!store.exists());
}
