use super::merged_set::*;
use super::*;
use std::path::Path;
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

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    import_split_set(&mut connection, &progress_bar, &tmp_directory).await;
    let system = rebuild(&mut connection, &progress_bar, "NON_MERGED").await;
    assert_eq!(system.merging, Merging::NonMerged as i64);

    // when
    let system = rebuild(&mut connection, &progress_bar, "MERGED").await;

    // then the clones' copies of their parent's files are gone with their ZIPs
    assert_merged(&mut connection, &system).await;
    assert_checks(&mut connection, &progress_bar).await;
}
