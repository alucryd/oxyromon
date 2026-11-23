use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20230618) (RVZ).dat").await;

    let romfile_path = env.tmp_directory.join("Test Game (USA).ore");
    ore_rs::compress(
        &env.fixture_source("Test Game (USA).iso"),
        &romfile_path,
        &ore_rs::Options::default(),
        &[],
        &mut |_| {},
    )
    .unwrap();
    env.import_roms(&[&romfile_path]).await;

    // when
    env.run(&["check-roms", "-a"]).await.unwrap();

    // then
    let romfile = env.single_romfile().await;
    assert!(!romfile.path.contains("/Trash/"));
    assert!(env.romfile_path(&romfile).is_file());
}
