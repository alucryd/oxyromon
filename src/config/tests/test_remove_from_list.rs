use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;

    let key = "DISCARD_FLAGS";

    set_list(&mut env.connection, key, &[String::from("item1")], None).await;

    // when
    remove_from_list(&mut env.connection, &env.progress_bar, key, "item1", None).await;
    let list = get_list(&mut env.connection, key, None).await;

    // then
    assert_eq!(list.len(), 0);
}
