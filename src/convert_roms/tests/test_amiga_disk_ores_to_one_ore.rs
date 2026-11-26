use crate::test_support::TestEnv;

const DISKS: [&str; 2] = [
    "Test Game (Europe) (Disk 1).ipf",
    "Test Game (Europe) (Disk 2).ipf",
];

#[tokio::test]
async fn test() {
    // given a game of two disks an ORE each, as earlier versions wrote them
    let mut env = TestEnv::with_dat("Commodore - Amiga (20261125).dat").await;
    let staging = env.tmp_directory.join("staging");
    std::fs::create_dir(&staging).unwrap();
    let options = ore_rs::Options {
        platform: Some("amiga"),
        ..Default::default()
    };
    let mut ores = Vec::new();
    for disk in DISKS {
        let ore = staging.join(disk).with_extension("ore");
        ore_rs::compress(&env.fixture_source(disk), &ore, &options, &[], &mut |_| {}).unwrap();
        ores.push(ore);
    }
    env.import_roms(&[&ores[0], &ores[1]]).await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    assert_eq!(env.romfiles().await.len(), 2);

    // when converted to ORE again, without recompressing
    env.run(&["convert-roms", "-f", "ORE", "-a", "-c"])
        .await
        .unwrap();

    // then the game is one ORE, each disk a file of it, taken whole
    let roms = env.roms_with_romfiles(&system).await;
    assert_eq!(roms.len(), 2);
    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 1);
    let path = system_directory.join("Test Game (Europe).ore");
    assert_eq!(romfiles[0].path, env.stored_path(&path));
    assert!(
        roms.iter()
            .all(|rom| rom.romfile_id == Some(romfiles[0].id))
    );
    env.assert_set(&path, "amiga", &DISKS);
    for disk in DISKS {
        assert!(!system_directory.join(disk).with_extension("ore").exists());
    }
    env.run(&["check-roms", "-a"]).await.unwrap();
}
