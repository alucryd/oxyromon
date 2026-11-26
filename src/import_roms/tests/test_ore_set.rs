use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given a game of two disks, one ORE made elsewhere from its folder
    let mut env = TestEnv::with_dat("Commodore - Amiga (20261125).dat").await;
    let disks = [
        "Test Game (Europe) (Disk 1).ipf",
        "Test Game (Europe) (Disk 2).ipf",
    ];
    let folder = env.tmp_directory.join("Some Name");
    std::fs::create_dir(&folder).unwrap();
    for disk in disks {
        std::fs::copy(env.fixture_source(disk), folder.join(disk)).unwrap();
    }
    let ore = env.tmp_directory.join("Some Name.ore");
    let options = ore_rs::Options {
        platform: Some("amiga"),
        ..Default::default()
    };
    ore_rs::compress(&folder, &ore, &options, &[], &mut |_| {}).unwrap();

    // when
    env.import_roms(&[&ore]).await;

    // then it is the game's, both disks in it, under the game's name
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    let path = system_directory.join("Test Game (Europe).ore");
    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 1);
    assert_eq!(romfiles[0].path, env.stored_path(&path));
    let roms = env.roms_with_romfiles(&system).await;
    assert_eq!(roms.len(), 2);
    assert!(
        roms.iter()
            .all(|rom| rom.romfile_id == Some(romfiles[0].id))
    );
    env.assert_set(&path, "amiga", &disks);
    env.run(&["check-roms", "-a"]).await.unwrap();
}
