use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    // the whole dependency + statistics report runs without error
    main(&mut env.connection, &env.progress_bar).await.unwrap();

    // and it reported against the imported database
    assert_eq!(count_systems(&mut env.connection).await, 1);
    assert!(count_games(&mut env.connection).await > 0);
    assert!(count_roms(&mut env.connection).await > 0);
}
