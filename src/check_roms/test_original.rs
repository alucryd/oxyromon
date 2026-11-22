use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;
    let romfile_path = env.fixture("Test Game (USA, Europe).rom").await;
    env.import_roms(&[&romfile_path]).await;

    // when: the same check, asked to recalculate sizes
    env.run(&["check-roms", "-a", "--size"]).await.unwrap();

    // then
    let romfile = env.single_romfile().await;
    assert!(!romfile.path.contains("/Trash/"));
    assert!(env.romfile_path(&romfile).is_file());
}
