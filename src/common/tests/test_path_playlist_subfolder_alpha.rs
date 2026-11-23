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

    // when
    let path = game
        .get_playlist_path(&mut env.connection, &system, &Some(SubfolderScheme::Alpha))
        .await
        .unwrap();

    // then
    assert_eq!(
        path,
        env.rom_directory
            .join(system.name)
            .join("G")
            .join("game name.m3u")
    );
}
