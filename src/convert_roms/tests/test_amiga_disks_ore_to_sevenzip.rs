use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given a game of two disks, an ORE each
    let mut env = TestEnv::with_dat("Commodore - Amiga (20261125).dat").await;
    let archive = env.fixture("Test Game (Europe).7z").await;
    env.import_roms(&[&archive]).await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();

    // when
    env.run(&["convert-roms", "-f", "7Z", "-a", "-c"])
        .await
        .unwrap();

    // then both disks are in the one archive, its romfile the only one
    let roms = env.roms_with_romfiles(&system).await;
    assert_eq!(roms.len(), 2);
    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 1);
    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        env.stored_path(&system_directory.join("Test Game (Europe).7z"))
    );
    assert!(roms.iter().all(|rom| rom.romfile_id == Some(romfile.id)));
    for disk in 1..=2 {
        let ore = system_directory.join(format!("Test Game (Europe) (Disk {disk}).ore"));
        assert!(!ore.exists());
    }
    env.run(&["check-roms", "-a"]).await.unwrap();
}
