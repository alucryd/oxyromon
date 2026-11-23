use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;

    let test_directory = get_canonicalized_path(&String::from("tests"))
        .await
        .unwrap();
    let key = "TEST_DIRECTORY";

    // when
    set_directory(&mut env.connection, key, &test_directory, None).await;

    let directory = get_directory(&mut env.connection, key, None).await.unwrap();

    // then
    assert_eq!(directory, test_directory);
}
