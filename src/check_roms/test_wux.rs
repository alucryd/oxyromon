use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261025) (WUD).dat").await;
    let romfile_path = env.fixture("Test Game (USA).wux").await;
    env.import_roms(&[&romfile_path]).await;

    // when
    env.run(&["check-roms", "-a"]).await.unwrap();

    // then
    let romfile = env.single_romfile().await;
    assert!(!romfile.path.contains("/Trash/"));
    assert!(env.romfile_path(&romfile).is_file());
}
