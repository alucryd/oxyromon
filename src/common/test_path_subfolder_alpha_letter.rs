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
    let rom = Rom {
        id: 1,
        name: String::from("rom name.rom"),
        bios: false,
        disk: false,
        size: 1,
        crc: Some(String::from("")),
        md5: Some(String::from("")),
        sha1: Some(String::from("")),
        rom_status: None,
        game_id: 1,
        romfile_id: Some(1),
        parent_id: None,
        original: true,
    };
    let romfile = Romfile {
        id: 1,
        path: String::from("romfile.rom"),
        size: 0,
        parent_id: None,
        romfile_type: RomfileType::Romfile as i64,
    };

    // when
    let path = romfile
        .as_common(&mut connection)
        .await
        .unwrap()
        .get_sorted_path(
            &mut connection,
            &system,
            &game,
            &rom,
            &Some(SubfolderScheme::Alpha),
            &None,
        )
        .await
        .unwrap();

    // then
    assert_eq!(
        path,
        rom_directory
            .path()
            .join(system.name)
            .join("G")
            .join("rom name.rom")
    );
}
