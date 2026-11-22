use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let key = "DISCARD_FLAGS";

    // when
    set_list(
        &mut env.connection,
        key,
        &[String::from("item1"), String::from("item2")],
        None,
    )
    .await;

    let list = get_list(&mut env.connection, key, None).await;

    // then
    assert_eq!(list.len(), 2);
    assert_eq!(list.first().unwrap(), "item1");
    assert_eq!(list.get(1).unwrap(), "item2");
}
