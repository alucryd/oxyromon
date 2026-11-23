use crate::database::*;
use crate::util::*;
use super::*;
use crate::test_support::TestEnv;
use std::path::Path;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261114) (Game Boy).dat").await;

    let romfile_path = env.tmp_directory.join("Test Game (USA, Europe).gb.7z");
    tokio::fs::copy(
        env.fixture_source("Test Game (USA, Europe).gb.7z"),
        &romfile_path,
    )
    .await
    .unwrap();
    env.import_roms(&[&romfile_path]).await;

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();
    let ore_path = system_directory.join("Test Game (USA, Europe).ore");
    let store = system_directory.join(ore_rs::STORE_DIR);
    let rom = std::fs::read(env.fixture_source("Test Game (USA, Europe).gb")).unwrap();
    let thin = |path: &Path| {
        ore_rs::Reader::inspect(path)
            .unwrap()
            .store_uuid()
            .is_some()
    };
    let reads_back = |path: &Path| {
        let mut reader = ore_rs::Reader::open(path).unwrap();
        let mut bytes = vec![0; rom.len()];
        reader.read_at(0, &mut bytes).unwrap();
        bytes == rom
    };

    // a store written with parity
    set_bool(&mut env.connection, "ORE_STORE", true, Some(system.id)).await;
    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();
    assert!(thin(&ore_path));
    let size = |dir: &Path| -> u64 {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().metadata().unwrap().len())
            .sum()
    };
    let with_parity = size(&store);

    // when its parity is turned off and the store compacted
    set_integer(&mut env.connection, "ORE_STORE_PARITY", 0, Some(system.id)).await;
    env.run(&["compact-roms", "-a"]).await.unwrap();

    // then it is sealed again without it, and the game reads on
    assert!(size(&store) < with_parity);
    assert!(reads_back(&ore_path));

    // and recompressed, it reads on still
    env.run(&["compact-roms", "-a", "-r"]).await.unwrap();
    assert!(reads_back(&ore_path));
    assert_eq!(find_romfiles(&mut env.connection).await.len(), 1);
}
