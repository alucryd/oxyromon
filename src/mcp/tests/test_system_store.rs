//! The per-system store numbers as the MCP tells them: the `get_system`
//! payload carries the system's own part of the savings, not the store's
//! whole, whenever its games live in a readable store.

use crate::config::{set_bool, set_string};
use crate::database::find_system_by_id_opt;
use crate::mcp::read::build_system_detail;
use crate::test_support::TestEnv;

/// A game converted to an ORE in its system's store: the payload shows
/// its own numbers — alone in the store it saves nobody's dedup, and
/// pays its own part of the parity, close to the store's footers but not
/// the store's whole.
#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261114) (Game Boy).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe).gb").await;
    env.import_roms(&[&romfile_path]).await;

    env.run(&["sort-roms", "-a", "-y", "-o", "US", "-r", "JP"])
        .await
        .unwrap();

    // and the game's ORE in the system's store
    let system = env.system().await;
    set_bool(&mut env.connection, "ORE_STORE", true, Some(system.id)).await;
    set_string(
        &mut env.connection,
        "ORE_STORE_BATCH",
        "game",
        Some(system.id),
    )
    .await;
    env.run(&["convert-roms", "-f", "ORE", "-a", "-c"])
        .await
        .unwrap();

    // when the MCP builds the payload of `get_system` for the system
    let system = find_system_by_id_opt(&mut env.connection, system.id)
        .await
        .expect("the system is still there");
    let detail = build_system_detail(&mut env.connection, &system)
        .await
        .unwrap();

    // then the savings are the game's own: one game alone in the store,
    // its chunks shared with nobody — no dedup to save it, and its part
    // of the parity
    let system_directory = env.system_directory(&system).await;
    let store = system_directory.join(ore_rs::STORE_DIR);
    let stats = ore_rs::store_stats(&store).unwrap();
    assert_eq!(
        detail.store_dedup_saved.unwrap(),
        0,
        "one game alone in the store saves nobody's dedup"
    );
    let paid = detail.store_parity_cost.unwrap();
    assert!(paid > 0, "the game pays its part of the parity");
    assert!(
        (stats.parity as i64 - paid).abs() < stats.parity as i64 / 8,
        "{paid} against the store's {}",
        stats.parity
    );
    assert!(detail.total_actual_size > 0);
}
