use crate::database::update_romfile;
use crate::test_support::TestEnv;

const DISKS: [&str; 2] = [
    "Test Game (Europe) (Disk 1).ipf",
    "Test Game (Europe) (Disk 2).ipf",
];

#[tokio::test]
async fn test() {
    // given a game of two disks, a romfile each, both of which sort under
    // the game's name, as zips do
    let mut env = TestEnv::with_dat("Commodore - Amiga (20261125).dat").await;
    env.import_disk_ores("amiga", &DISKS).await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    for romfile in env.romfiles().await {
        let ore = env.rom_directory.join(&romfile.path);
        let zip = ore.with_extension("zip");
        std::fs::rename(&ore, &zip).unwrap();
        let stored = env.stored_path(&zip);
        update_romfile(
            &mut env.connection,
            romfile.id,
            &stored,
            romfile.size as u64,
        )
        .await;
    }
    let before = env.romfiles().await;
    assert_eq!(before.len(), 2);

    // when it is sorted into 1G1R
    env.add_to_list("REGIONS_ONE", "EU", system.id).await;
    let error = env.run(&["sort-roms", "-a", "-y"]).await.unwrap_err();

    // then the sort fails before moving anything, and both romfiles stay
    assert!(
        error.to_string().contains("would both be moved to"),
        "{error}"
    );
    let after = env.romfiles().await;
    assert_eq!(
        after
            .iter()
            .map(|romfile| &romfile.path)
            .collect::<Vec<_>>(),
        before
            .iter()
            .map(|romfile| &romfile.path)
            .collect::<Vec<_>>()
    );
    for romfile in &after {
        assert!(
            env.rom_directory.join(&romfile.path).is_file(),
            "{}",
            romfile.path
        );
    }
    assert!(!system_directory.join("1G1R").exists());
}
