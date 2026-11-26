use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given a game of two disks, an ORE each, LZMA2 as a floppy's default
    let mut env = TestEnv::with_dat("Commodore - Amiga (20261125).dat").await;
    let archive = env.fixture("Test Game (Europe).7z").await;
    env.import_roms(&[&archive]).await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();

    // when recompressed with Zstandard
    env.set_string("ORE_COMPRESSION_ALGORITHM", "zstd", system.id)
        .await;
    env.run(&["convert-roms", "-f", "ORE", "-a", "-r", "-c"])
        .await
        .unwrap();

    // then both disks are, not just the first
    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 2);
    for disk in 1..=2 {
        let path = system_directory.join(format!("Test Game (Europe) (Disk {disk}).ore"));
        assert_eq!(
            ore_rs::Reader::open(&path).unwrap().codec(),
            ore_rs::Codec::Zstd
        );
    }
    env.run(&["check-roms", "-a"]).await.unwrap();
}
