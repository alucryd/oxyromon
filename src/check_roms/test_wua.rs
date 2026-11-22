use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env =
        TestEnv::with_dat("Test System (20261025) (WUD).dat").await;

    // a lossless WUA, its update partition in the store beside it
    let staging = env.tmp_directory.join("staging");
    fs::create_dir(&staging).await.unwrap();
    let wua_path = staging.join("Test Game (USA).wua");
    let keys = wux_rs::load_keys(
        &env.fixture_source("Test Game (USA).keys.txt"),
    )
    .unwrap();
    wux_rs::to_wua(
        &[&env.fixture_source("Test Game (USA).wux")],
        &wua_path,
        &keys,
        Some(&staging.join(wux_rs::STORE_DIR)),
        &mut |_| {},
    )
    .unwrap();
    env.import_roms(&[&staging], &[]).await;

    // when
    env.run(&["check-roms", "-a"]).await.unwrap();

    // then
    let romfile = env.single_romfile().await;
    assert!(!romfile.path.contains("/Trash/"));
    assert!(env.romfile_path(&romfile).is_file());
}
