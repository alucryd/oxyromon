use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given a CD's ORE, and a file the database does not know already under
    // the game's name
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;
    let ore = env
        .tmp_directory
        .join("Test Game (USA, Europe) (Multiple Tracks).ore");
    ore_rs::compress(
        &env.fixture_source("Test Game (USA, Europe) (Multiple Tracks).cue"),
        &ore,
        &ore_rs::Options::default(),
        &[],
        &mut |_| {},
    )
    .unwrap();
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    std::fs::create_dir_all(&system_directory).unwrap();
    let taken = system_directory.join("Test Game (USA, Europe) (CUE BIN).ore");
    std::fs::write(&taken, b"another file").unwrap();

    // when
    env.import_roms(&[&ore]).await;

    // then neither is moved nor replaced, and nothing is imported
    assert_eq!(std::fs::read(&taken).unwrap(), b"another file");
    assert!(ore.is_file());
    assert!(env.romfiles().await.is_empty());
}
