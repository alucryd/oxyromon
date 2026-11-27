use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given a game of two disks, one ORE, in the system's store
    let mut env = TestEnv::with_dat("Commodore - Amiga (20261125).dat").await;
    let archive = env.fixture("Test Game (Europe).7z").await;
    env.import_roms(&[&archive]).await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    env.set_bool("ORE_STORE", true, system.id).await;
    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();

    // when it is sorted into 1G1R
    env.add_to_list("REGIONS_ONE", "EU", system.id).await;
    env.run(&["sort-roms", "-a", "-y"]).await.unwrap();

    // then the game's one ORE goes, under its name, and reads
    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 1);
    let path = system_directory.join("1G1R").join("Test Game (Europe).ore");
    assert_eq!(romfiles[0].path, env.stored_path(&path));
    env.assert_set(
        &path,
        "amiga",
        &[
            "Test Game (Europe) (Disk 1).ipf",
            "Test Game (Europe) (Disk 2).ipf",
        ],
    );
    env.run(&["check-roms", "-a"]).await.unwrap();
}
