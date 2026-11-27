use super::*;
use crate::test_support::TestEnv;

const DISKS: [&str; 2] = [
    "Test Game (Europe) (Disk 1).ipf",
    "Test Game (Europe) (Disk 2).ipf",
];

#[tokio::test]
async fn test() {
    // given a game of two disks an ORE each, the first's moved to the
    // game's name, as sort-roms once did
    let mut env = TestEnv::with_dat("Commodore - Amiga (20261125).dat").await;
    env.import_disk_ores("amiga", &DISKS).await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    let path = system_directory.join("Test Game (Europe).ore");
    let first = system_directory.join(DISKS[0]).with_extension("ore");
    std::fs::rename(&first, &path).unwrap();
    let romfile = env
        .romfiles()
        .await
        .into_iter()
        .find(|romfile| romfile.path == env.stored_path(&first))
        .unwrap();
    let stored = env.stored_path(&path);
    update_romfile(
        &mut env.connection,
        romfile.id,
        &stored,
        romfile.size as u64,
    )
    .await;

    // when
    env.run(&["convert-roms", "-f", "ORE", "-a", "-c"])
        .await
        .unwrap();

    // then the game is one ORE under its name, which it took over
    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 1);
    assert_eq!(romfiles[0].path, env.stored_path(&path));
    env.assert_set(&path, "amiga", &DISKS);
    assert_eq!(std::fs::read_dir(&system_directory).unwrap().count(), 1);
    env.run(&["check-roms", "-a"]).await.unwrap();
}
