use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let system = env.system().await;

    // when
    set_bool(&mut env.connection, "PREFER_PARENTS", true, None).await;
    set_bool(
        &mut env.connection,
        "PREFER_PARENTS",
        false,
        Some(system.id),
    )
    .await;

    // then
    assert!(!get_bool(&mut env.connection, "PREFER_PARENTS", Some(system.id)).await);
    assert!(get_bool(&mut env.connection, "PREFER_PARENTS", None).await);
}
