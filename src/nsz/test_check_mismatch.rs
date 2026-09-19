use super::super::database::establish_connection;
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

    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let nsz_path = tmp_directory.path().join("Test Game (USA).nsz");
    fs::copy(test_directory.join("Test Game (USA).nsz"), &nsz_path)
        .await
        .unwrap();
    let nsz = CommonRomfile::from_path(&nsz_path)
        .unwrap()
        .as_nsz()
        .unwrap();

    let rom = Rom {
        id: 1,
        name: "Test Game (USA).nsp".to_string(),
        bios: false,
        disk: false,
        size: 40946,
        crc: None,
        md5: None,
        sha1: Some("0000000000000000000000000000000000000000".to_string()),
        rom_status: None,
        game_id: 1,
        romfile_id: None,
        parent_id: None,
        original: true,
    };

    // when
    let result = nsz
        .check(&mut connection, &progress_bar, &None, &[&rom])
        .await;

    // then
    assert!(result.is_err());
}
