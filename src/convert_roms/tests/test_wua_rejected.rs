use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    let mut env = TestEnv::with_dat("Test System (20261025) (WUD).dat").await;
    let system = env.system().await;

    assert!(
        subcommand()
            .try_get_matches_from(["convert-roms", "-f", "WUA", "-a"])
            .is_err()
    );
    assert!(!ALL_FORMATS.contains(&"WUA"));

    set_setting(
        &mut env.connection,
        &env.progress_bar,
        "PREFER_FORMAT",
        "WUA",
        Some(system.id),
    )
    .await
    .unwrap();
    assert_eq!(
        get_string(&mut env.connection, "PREFER_FORMAT", Some(system.id)).await,
        None
    );
}
