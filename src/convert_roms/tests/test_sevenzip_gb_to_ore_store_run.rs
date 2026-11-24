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

    // when the store is on, the run one batch: written straight into it
    set_bool(&mut env.connection, "ORE_STORE", true, Some(system.id)).await;
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a", "-c"]);
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();

    // then the ORE is thin, the database points to it, and the archive is gone
    assert!(thin(&ore_path));
    assert!(reads_back(&ore_path));
    assert!(
        !system_directory
            .join("Test Game (USA, Europe).gb.7z")
            .exists()
    );
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    assert!(romfiles[0].path.ends_with("Test Game (USA, Europe).ore"));
    // and nothing is left of the batch
    assert!(
        std::fs::read_dir(&store)
            .unwrap()
            .all(|entry| { !entry.unwrap().path().to_string_lossy().ends_with(".part") })
    );
}
