use super::super::config::*;
use super::super::database::*;
use super::*;
use async_std::path::PathBuf;
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

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    set_rom_directory(PathBuf::from(rom_directory.path()));
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    set_tmp_directory(PathBuf::from(tmp_directory.path()));

    set_bool(&mut connection, "GROUP_SUBSYSTEMS", false).await;

    let system = System {
        id: 1,
        name: String::from("Nintendo - Nintendo 64 (BigEndian)"),
        description: String::from(""),
        version: String::from(""),
        url: None,
        arcade: false,
        complete: false,
        merging: Merging::NonMerged as i64,
    };

    // when
    let system_directory = get_system_directory(&mut connection, &progress_bar, &system)
        .await
        .unwrap();

    // then
    assert_eq!(
        system_directory.file_name().unwrap().to_str().unwrap(),
        "Nintendo - Nintendo 64 (BigEndian)"
    );
}
