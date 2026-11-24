use super::*;
use crate::test_support::TestEnv;
use std::path::Path;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261114) (Game Boy).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe).gb.7z").await;
    env.import_roms(&[&romfile_path]).await;

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
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

    // when the store is on, a game at a time then a run at a time
    set_bool(&mut env.connection, "ORE_STORE", true, Some(system.id)).await;
    set_string(
        &mut env.connection,
        "ORE_STORE_BATCH",
        "game",
        Some(system.id),
    )
    .await;
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a", "-c"]);
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();

    // then the ORE is thin, its chunks in the system's store
    assert!(store.is_dir());
    assert!(thin(&ore_path));
    assert!(reads_back(&ore_path));
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    // and it weighs its share of the store, not its file alone
    let size = crate::ore::actual_size(&mut env.connection, &romfiles)
        .await
        .unwrap();
    assert_eq!(size, ore_rs::Reader::open(&ore_path).unwrap().sizes().share);
    assert!(size > romfiles[0].size as u64);
    env.run(&["check-roms", "-a"]).await.unwrap();

    // when the store is off, it is standalone again, and the run batched
    set_bool(&mut env.connection, "ORE_STORE", false, Some(system.id)).await;
    set_string(
        &mut env.connection,
        "ORE_STORE_BATCH",
        "run",
        Some(system.id),
    )
    .await;
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a"]);
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();
    assert!(!thin(&ore_path));
    assert!(reads_back(&ore_path));

    // and back in, a run at a time
    set_bool(&mut env.connection, "ORE_STORE", true, Some(system.id)).await;
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a"]);
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();
    assert!(thin(&ore_path));
    assert!(reads_back(&ore_path));

    // then a purge never takes the store's files for foreign ones
    env.run(&["purge-roms", "-f", "-y"]).await.unwrap();
    assert!(reads_back(&ore_path));
    assert!(store.join("store.index").is_file());
    assert_eq!(find_romfiles(&mut env.connection).await.len(), 1);

    // and a compaction keeps what the game uses
    env.run(&["compact-roms", "-a"]).await.unwrap();
    assert!(reads_back(&ore_path));
}
