use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given a legacy lossless WUA, its update partition in the legacy store
    // beside it
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

    // when
    env.run(&["check-roms", "-a"]).await.unwrap();

    // then
    let romfile = env.single_romfile().await;
    assert!(!romfile.path.contains("/Trash/"));
    assert!(env.romfile_path(&romfile).is_file());
}
