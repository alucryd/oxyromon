use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261025) (WUD).dat").await;

    // a lossless WUA, its update partition in the store beside it
    let staging = env.tmp_directory.join("staging");
    fs::create_dir(&staging).await.unwrap();
    let wua_path = staging.join("Test Game (USA).wua");
    let keys = wux_rs::load_keys(&env.fixture_source("Test Game (USA).keys.txt")).unwrap();
    wux_rs::to_wua(
        &[&env.fixture_source("Test Game (USA).wux")],
        &wua_path,
        &keys,
        Some(&staging.join(wux_rs::STORE_DIR)),
        &mut |_| {},
    )
    .unwrap();
    env.import_roms(&[&staging]).await;

    // the update partition, gone from the system's store: the WUA can't be
    // rebuilt, so it fails its check
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    for entry in wux_rs::store_entries(&system_directory.join("Test Game (USA).wua")).unwrap() {
        fs::remove_file(system_directory.join(wux_rs::STORE_DIR).join(entry))
            .await
            .unwrap();
    }

    // when
    env.run(&["check-roms", "-a"]).await.unwrap();

    // then
    let romfile = env.single_romfile().await;
    assert!(romfile.path.contains("/Trash/"));
    assert!(env.romfile_path(&romfile).is_file());
}
