use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given a game of two disks, one ORE made elsewhere from its folder, and
    // a file the database does not know already under the game's name
    let mut env = TestEnv::with_dat("Commodore - Amiga (20261125).dat").await;
    let folder = env.tmp_directory.join("Some Name");
    std::fs::create_dir(&folder).unwrap();
    for disk in [
        "Test Game (Europe) (Disk 1).ipf",
        "Test Game (Europe) (Disk 2).ipf",
    ] {
        std::fs::copy(env.fixture_source(disk), folder.join(disk)).unwrap();
    }
    let ore = env.tmp_directory.join("Some Name.ore");
    let options = ore_rs::Options {
        platform: Some("amiga"),
        ..Default::default()
    };
    ore_rs::compress(&folder, &ore, &options, &[], &mut |_| {}).unwrap();
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    std::fs::create_dir_all(&system_directory).unwrap();
    let taken = system_directory.join("Test Game (Europe).ore");
    std::fs::write(&taken, b"another file").unwrap();

    // when
    env.import_roms(&[&ore]).await;

    // then neither is moved nor replaced, and nothing is imported
    assert_eq!(std::fs::read(&taken).unwrap(), b"another file");
    assert!(ore.is_file());
    assert!(env.romfiles().await.is_empty());
}
