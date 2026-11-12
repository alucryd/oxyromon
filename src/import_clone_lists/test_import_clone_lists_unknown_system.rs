use super::super::config::{MUTEX, set_tmp_directory};
use super::*;
use std::io::Write;
use tempfile::{NamedTempFile, TempDir};

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let mut clone_list_file = NamedTempFile::new_in(tmp_directory.path()).unwrap();
    write!(
        clone_list_file,
        r#"{{"description": {{"name": "Unknown System (Redump)", "lastUpdated": "2026-11-11 00:00:00"}}, "variants": []}}"#
    )
    .unwrap();

    // when
    let imported = import_clone_list(
        &mut connection,
        &progress_bar,
        clone_list_file.path(),
        false,
    )
    .await
    .unwrap();

    // then
    assert_eq!(imported, None);
}
