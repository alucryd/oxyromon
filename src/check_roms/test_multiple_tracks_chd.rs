use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;
    let romfile_path = env
        .fixture("Test Game (USA, Europe) (Multiple Tracks).chd")
        .await;
    env.import_roms(&[&romfile_path]).await;

    // when
    env.run(&["check-roms", "-a"]).await.unwrap();

    // then
    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 1);
    for romfile in &romfiles {
        assert!(!romfile.path.contains("/Trash/"));
        assert!(env.romfile_path(romfile).is_file());
    }
}
