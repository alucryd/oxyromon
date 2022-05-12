use super::*;
use async_std::fs;
use async_std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};

#[async_std::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    let old_directory = PathBuf::from(&tmp_directory.path()).join("old");
    create_directory(&progress_bar, &old_directory, true)
        .await
        .unwrap();
    set_directory(&mut connection, "TEST_DIRECTORY", &old_directory).await;
    fs::remove_dir_all(&old_directory).await.unwrap();

    // when
    get_directory(&mut connection, "TEST_DIRECTORY").await;
    let new_directory = PathBuf::from(&tmp_directory.path()).join("new");
    create_directory(&progress_bar, &new_directory, true)
        .await
        .unwrap();
    set_directory(&mut connection, "TEST_DIRECTORY", &new_directory).await;

    // then
    let directory = get_directory(&mut connection, "TEST_DIRECTORY").await;
    assert!(directory.is_some());
    assert!(&directory.unwrap().as_os_str() == &new_directory.as_os_str());
}
