use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261025) (WUD).dat").await;

    // a lossless WUA, imported with its update partition
    let staging = env.tmp_directory.join("staging");
    fs::create_dir(&staging).await.unwrap();
    let keys = wux_rs::load_keys(&env.fixture_source("Test Game (USA).keys.txt")).unwrap();
    wux_rs::to_wua(
        &[&env.fixture_source("Test Game (USA).wux")],
        &staging.join("Test Game (USA).wua"),
        &keys,
        Some(&staging.join(STORE_DIR)),
        &mut |_| {},
    )
    .unwrap();
    env.import_roms(&[&staging]).await;

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();
    let wua = system_directory.join("Test Game (USA).wua");
    let store = system_directory.join(STORE_DIR);
    let entry = store.join(&wux_rs::store_entries(&wua).unwrap()[0]);
    // and an update partition no WUA needs any more
    let stray = store.join("0000000000000000000000000000000000000000.zst");
    fs::write(&stray, b"stray").await.unwrap();
    // but the WUA is cut short, so what it needs can't be read
    let length = fs::metadata(&wua).await.unwrap().len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&wua)
        .unwrap()
        .set_len(length / 2)
        .unwrap();

    // when
    purge_foreign_romfiles(&mut env.connection, &env.progress_bar, true)
        .await
        .unwrap();

    // then every update partition is kept
    assert!(wua.is_file());
    assert!(entry.is_file());
    assert!(stray.is_file());
}
