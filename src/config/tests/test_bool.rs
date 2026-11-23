use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let key = "TEST_BOOLEAN";

    // when
    set_bool(&mut env.connection, key, true, None).await;
    let bool = get_bool(&mut env.connection, key, None).await;

    // then
    assert!(bool);
}
