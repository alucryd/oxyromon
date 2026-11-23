use super::*;
use crate::test_support::TestEnv;
use std::path::PathBuf;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let old_directory = PathBuf::from(&env.tmp_directory).join("old");
    create_directory(&env.progress_bar, &old_directory, true)
        .await
        .unwrap();
    set_directory(&mut env.connection, "TEST_DIRECTORY", &old_directory, None).await;
    fs::remove_dir_all(&old_directory).await.unwrap();

    // when
    get_directory(&mut env.connection, "TEST_DIRECTORY", None).await;
    let new_directory = PathBuf::from(&env.tmp_directory).join("new");
    create_directory(&env.progress_bar, &new_directory, true)
        .await
        .unwrap();
    set_directory(&mut env.connection, "TEST_DIRECTORY", &new_directory, None).await;

    // then
    let directory = get_directory(&mut env.connection, "TEST_DIRECTORY", None).await;
    assert!(directory.is_some());
    assert_eq!(directory.unwrap().as_os_str(), new_directory.as_os_str());
}
