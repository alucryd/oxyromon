use tempfile::{NamedTempFile, TempDir};

use super::*;

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

    let system = System {
        id: 1,
        name: String::from("Test System"),
        custom_name: None,
        description: String::from(""),
        version: String::from(""),
        url: Some(String::from("")),
        arcade: false,
        merging: Merging::Split as i64,
        completion: 0,
        custom_extension: None,
    };
    let game = Game {
        id: 1,
        name: String::from("game name"),
        description: String::from(""),
        comment: None,
        external_id: None,
        device: false,
        bios: false,
        jbfolder: false,
        regions: String::from(""),
        sorting: Sorting::AllRegions as i64,
        completion: 0,
        system_id: 1,
        parent_id: None,
        bios_id: None,
        playlist_id: None,
    };

    // when
    let path = game
        .get_playlist_path(&mut connection, &system, &None)
        .await
        .unwrap();

    // then
    assert_eq!(
        path,
        rom_directory.path().join(system.name).join("game name.m3u")
    );
}
