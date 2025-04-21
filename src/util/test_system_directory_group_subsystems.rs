use super::super::database::*;
use super::*;
use std::path::PathBuf;
use tempfile::{NamedTempFile, TempDir};

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let system = System {
        id: 1,
        name: String::from("Nintendo - Nintendo 64 (BigEndian)"),
        custom_name: None,
        description: String::from(""),
        version: String::from(""),
        url: None,
        arcade: false,
        completion: 0,
        merging: Merging::NonMerged as i64,
    };

    // when
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    // then
    assert_eq!(
        system_directory.file_name().unwrap().to_str().unwrap(),
        "Nintendo - Nintendo 64"
    );
}
