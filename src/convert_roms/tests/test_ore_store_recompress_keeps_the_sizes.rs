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
    let share = |path: &Path| ore_rs::Reader::open(path).unwrap().sizes().share;
    let thin = |path: &Path| {
        ore_rs::Reader::inspect(path)
            .unwrap()
            .store_uuid()
            .is_some()
    };

    // a game in the store, which shares nothing with anybody: what its
    // references say is what its sizes weigh
    set_bool(&mut env.connection, "ORE_STORE", true, Some(system.id)).await;
    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();
    assert!(thin(&ore_path));
    let share_before = share(&ore_path);
    assert_eq!(ore_rs::store_stats(&store).unwrap().dedup_saved, 0);

    // when it is recompressed in its place: the run takes the thin ORE
    // out, writes a standalone, and the sync moves it back in, naming the
    // very chunks it named before
    let written = std::fs::metadata(&ore_path).unwrap().modified().unwrap();
    env.run(&["convert-roms", "-f", "ORE", "-a", "-r"])
        .await
        .unwrap();
    assert!(
        std::fs::metadata(&ore_path).unwrap().modified().unwrap() > written,
        "the recompression wrote nothing"
    );

    // then the game is thin again and the store counts it once: no copy
    // of itself left holding references, no dedup it never made
    assert!(thin(&ore_path));
    let stats = ore_rs::store_stats(&store).unwrap();
    assert_eq!(stats.dedup_saved, 0);
    assert_eq!(share(&ore_path), share_before);
    env.run(&["check-roms", "-a"]).await.unwrap();
}
