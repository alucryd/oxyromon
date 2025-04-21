use super::super::config::{MUTEX, set_rom_directory};
use super::super::database::*;
use super::*;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    let rom_directory =
        set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;

    let romfile_path = rom_directory.join("Test Game (USA, Europe).rom");
    fs::copy(
        test_directory.join("Test Game (USA, Europe).rom"),
        &romfile_path,
    )
    .await
    .unwrap();

    // when
    purge_foreign_romfiles(&mut connection, &progress_bar, true)
        .await
        .unwrap();

    // then
    assert!(!romfile_path.is_file());
}
