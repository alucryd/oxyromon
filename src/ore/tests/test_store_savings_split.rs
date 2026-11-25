//! Two systems grouped in one directory share one store: the savings
//! fields split its dedup and its parity by game, so each system shows
//! its own numbers, and the parts add up to the store's whole.

use super::*;
use crate::database::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given a base system with two subsystems grouped in its directory,
    // a Wii U disc under one and its eShop update under the other
    let mut env = TestEnv::new().await;
    env.import_dats(&[
        "Test System (20200721).dat",
        "Test System (20261025) (WUD).dat",
        "Test System (20261025) (CDN).dat",
    ])
    .await;
    let systems = find_systems(&mut env.connection).await;
    let base = systems.iter().find(|s| s.name == "Test System").unwrap();
    let wud = systems
        .iter()
        .find(|s| s.name == "Test System (WUD)")
        .unwrap();
    let cdn = systems
        .iter()
        .find(|s| s.name == "Test System (CDN)")
        .unwrap();
    set_bool(&mut env.connection, "ORE_STORE", true, Some(base.id)).await;

    let disc = env.fixture("Test Game (USA).wud").await;
    env.run(&["import-roms", disc.to_str().unwrap()])
        .await
        .unwrap();
    let update = env.fixture("Test Game (USA) (Update)").await;
    env.run(&["import-roms", update.to_str().unwrap()])
        .await
        .unwrap();

    // when every game of both systems becomes a thin ORE of their one store
    env.run(&["convert-roms", "-f", "ORE", "-a", "-c"])
        .await
        .unwrap();

    // then each system's savings are its own, not the store's shown twice
    let store = env.system_directory(base).await.join(ore_rs::STORE_DIR);
    let stats = ore_rs::store_stats(&store).unwrap();
    let wud_romfiles = find_romfiles_by_system_id(&mut env.connection, wud.id).await;
    let cdn_romfiles = find_romfiles_by_system_id(&mut env.connection, cdn.id).await;
    let wud_savings = crate::ore::store_savings(&mut env.connection, &wud_romfiles)
        .await
        .expect("the disc's ORE lives in a readable store");
    let cdn_savings = crate::ore::store_savings(&mut env.connection, &cdn_romfiles)
        .await
        .expect("the update's ORE lives in the same store");
    assert!(
        wud_savings.parity > 0 && cdn_savings.parity > 0,
        "a game of each system pays parity"
    );
    assert!(
        wud_savings.parity < stats.parity && cdn_savings.parity < stats.parity,
        "no system pays for the whole store on its own"
    );
    assert_ne!(
        wud_savings, cdn_savings,
        "two different games pay differently"
    );

    // and the two parts add up to the store's whole: the dedup to the
    // rounding of the shares, the parity give or take the envelope headers
    // and the room no game lies in
    let saved = wud_savings.dedup_saved + cdn_savings.dedup_saved;
    assert!(
        saved.abs_diff(stats.dedup_saved) < 0x400,
        "{saved} against {}",
        stats.dedup_saved
    );
    let paid = wud_savings.parity + cdn_savings.parity;
    assert!(
        paid.abs_diff(stats.parity) < stats.parity / 8,
        "{paid} against {}",
        stats.parity
    );
}
