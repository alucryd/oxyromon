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

    let system = import_split_set(&mut connection, &progress_bar, &tmp_directory).await;
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();
    let system = rebuild(&mut connection, &progress_bar, "MERGED").await;
    assert_merged(&mut connection, &system).await;

    // when
    let system = rebuild(&mut connection, &progress_bar, "NON_MERGED").await;

    // then each clone has its own files and its parent's again
    assert_eq!(system.merging, Merging::NonMerged as i64);
    assert_eq!(
        entry_names(&system_directory.join("Test Parent.zip")),
        ["Test Common.rom", "Test Program.rom"]
    );
    assert_eq!(
        entry_names(&system_directory.join("Test Clone 1.zip")),
        ["Test Clones.rom", "Test Common.rom", "Test Program.rom"]
    );
    assert_eq!(
        entry_names(&system_directory.join("Test Clone 2.zip")),
        ["Test Clones.rom", "Test Common.rom"]
    );
    for game in find_games_by_system_id(&mut connection, system.id).await {
        assert_eq!(game.completion, Completion::Full as i64, "{}", game.name);
    }
    assert_checks(&mut connection, &progress_bar).await;
}
