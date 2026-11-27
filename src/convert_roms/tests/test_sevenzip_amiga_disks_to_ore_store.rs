use super::*;
use crate::test_support::TestEnv;

const DISKS: [&str; 2] = [
    "Test Game (Europe) (Disk 1).ipf",
    "Test Game (Europe) (Disk 2).ipf",
];

#[tokio::test]
async fn test() {
    // given a game of two disks, from its archive, into the system's store, a run at a time
    let mut env = TestEnv::with_dat("Commodore - Amiga (20261125).dat").await;
    let archive = env.fixture("Test Game (Europe).7z").await;
    env.import_roms(&[&archive]).await;

    let system = env.system().await;
    assert_eq!(cartridge_platform(&system), Some("amiga"));
    let system_directory = env.system_directory(&system).await;
    env.set_bool("ORE_STORE", true, system.id).await;

    // when
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
    assert!(
        ore_rs::Reader::inspect(&path)
            .unwrap()
            .store_uuid()
            .is_some()
    );
    assert!(!system_directory.join("Test Game (Europe).7z").exists());
    env.run(&["check-roms", "-a"]).await.unwrap();
}
