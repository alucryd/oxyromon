use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env =
        TestEnv::with_dat("Test System (20261025) (CDN).dat").await;
    let update_directory = env.fixture("Test Game (USA) (Update)").await;
    env.import_roms(&[&update_directory], &[]).await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    // when
    env.run(&["check-roms", "-a"]).await.unwrap();

    // then every file is checked and passes, contents named without an
    // extension included
    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 6);
    assert!(romfiles.iter().all(|romfile| {
        romfile
            .path
            .starts_with("Test System/Test Game (USA) (Update)/")
    }));
    assert!(!system_directory.join("Trash").exists());
}
