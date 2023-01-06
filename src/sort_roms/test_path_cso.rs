use super::*;

#[async_std::test]
async fn test() {
    // given
    let test_directory = Path::new("tests");
    let system = System {
        id: 1,
        name: String::from("Test System"),
        description: String::from(""),
        version: String::from(""),
        url: Some(String::from("")),
        arcade: false,
        merging: Merging::Split as i64,
        complete: false,
    };
    let game = Game {
        id: 1,
        name: String::from("game name"),
        description: String::from(""),
        comment: None,
        external_id: None,
        bios: false,
        jbfolder: false,
        regions: String::from(""),
        sorting: Sorting::AllRegions as i64,
        complete: false,
        system_id: 1,
        parent_id: None,
        bios_id: None,
        playlist_id: None,
    };
    let rom = Rom {
        id: 1,
        name: String::from("rom name.iso"),
        bios: false,
        size: 1,
        crc: Some(String::from("")),
        md5: Some(String::from("")),
        sha1: Some(String::from("")),
        rom_status: None,
        game_id: 1,
        romfile_id: Some(1),
        parent_id: None,
    };
    let romfile = Romfile {
        id: 1,
        path: String::from("romfile.cso"),
        size: 0,
    };

    // when
    let path = compute_new_romfile_path(&system, &game, &rom, &romfile, &test_directory, "none")
        .await
        .unwrap();

    // then
    assert_eq!(path, test_directory.join("game name.cso"));
}
