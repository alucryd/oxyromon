use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given a game of two disks, one ORE
    let mut env = TestEnv::with_dat("Commodore - Amiga (20261125).dat").await;
    let archive = env.fixture("Test Game (Europe).7z").await;
    env.import_roms(&[&archive]).await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();

    // when
    env.run(&["convert-roms", "-f", "ORIGINAL", "-a", "-c"])
        .await
        .unwrap();

    // then both disks are back as they were, in a folder named after the
    // game, a romfile each
    let roms = env.roms_with_romfiles(&system).await;
    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 2);
    for rom in &roms {
        let romfile = romfiles
            .iter()
            .find(|romfile| Some(romfile.id) == rom.romfile_id)
            .unwrap();
        let path = system_directory.join("Test Game (Europe)").join(&rom.name);
        assert_eq!(romfile.path, env.stored_path(&path));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            std::fs::read(env.fixture_source(&rom.name)).unwrap()
        );
    }
    assert!(!system_directory.join("Test Game (Europe).ore").exists());
    env.run(&["check-roms", "-a"]).await.unwrap();
}
