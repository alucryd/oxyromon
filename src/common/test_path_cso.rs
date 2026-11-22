use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
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
        games_complete: 0,
        games_total: 0,
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
        categories: None,
    };
    let rom = Rom {
        id: 1,
        name: String::from("rom name.iso"),
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
        path: String::from("romfile.cso"),
        size: 0,
        parent_id: None,
        romfile_type: RomfileType::Romfile as i64,
    };

    // when
    let path = romfile
        .as_common(&mut env.connection)
        .await
        .unwrap()
        .get_sorted_path(
            &mut env.connection,
            &system,
            &game,
            &rom,
            &None,
            &Some(CSO_EXTENSION),
        )
        .await
        .unwrap();

    // then
    assert_eq!(
        path,
        env.rom_directory.join(system.name).join("game name.cso")
    );
}
