use super::super::rebuild_roms::merged_set::*;
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
    let system = rebuild(&mut connection, &progress_bar, "MERGED").await;

    // when
    let matches = subcommand().get_matches_from(["sort-roms", "-a", "-y"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then the clones do not drag their parent's ZIP to their own names
    assert_merged(&mut connection, &system).await;
}
