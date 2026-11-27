use crate::test_support::TestEnv;
use tempfile::TempDir;

#[tokio::test]
async fn test() {
    // given a title's ORE, made by one collection
    let elsewhere = TempDir::new().unwrap();
    let ore = elsewhere.path().join("Somewhere Else.ore");
    {
        let mut env = TestEnv::with_dat("Test System (20261025) (CDN).dat").await;
        let update_directory = env.fixture("Test Game (USA) (Update)").await;
        env.import_roms(&[&update_directory]).await;
        env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();
        let system = env.system().await;
        let system_directory = env.system_directory(&system).await;
        std::fs::copy(system_directory.join("Test Game (USA) (Update).ore"), &ore).unwrap();
    }

    // and another collection with a file it does not know under the game's
    // name
    let mut env = TestEnv::with_dat("Test System (20261025) (CDN).dat").await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    std::fs::create_dir_all(&system_directory).unwrap();
    let taken = system_directory.join("Test Game (USA) (Update).ore");
    std::fs::write(&taken, b"another file").unwrap();

    // when
    env.import_roms(&[&ore]).await;

    // then neither is moved nor replaced, and nothing is imported
    assert_eq!(std::fs::read(&taken).unwrap(), b"another file");
    assert!(ore.is_file());
    assert!(env.romfiles().await.is_empty());
}
