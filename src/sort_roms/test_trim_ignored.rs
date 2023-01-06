use super::*;

#[async_std::test]
async fn test() {
    // given
    let games = vec![
        Game {
            id: 1,
            name: String::from("Game (USA)"),
            description: String::from(""),
            comment: None,
            external_id: None,
            bios: false,
            jbfolder: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            complete: true,
            system_id: 1,
            parent_id: None,
            bios_id: None,
            playlist_id: None,
        },
        Game {
            id: 2,
            name: String::from("Game (USA) (Beta)"),
            description: String::from(""),
            comment: None,
            external_id: None,
            bios: false,
            jbfolder: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            complete: true,
            system_id: 1,
            parent_id: None,
            bios_id: None,
            playlist_id: None,
        },
        Game {
            id: 3,
            name: String::from("Game (USA) (Beta 1)"),
            description: String::from(""),
            comment: None,
            external_id: None,
            bios: false,
            jbfolder: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            complete: true,
            system_id: 1,
            parent_id: None,
            bios_id: None,
            playlist_id: None,
        },
        Game {
            id: 4,
            name: String::from("Game (USA) (Virtual Console, Switch Online)"),
            description: String::from(""),
            comment: None,
            external_id: None,
            bios: false,
            jbfolder: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            complete: true,
            system_id: 1,
            parent_id: None,
            bios_id: None,
            playlist_id: None,
        },
    ];

    let ignored_releases = vec!["Beta"];
    let ignored_flags = vec!["Virtual Console"];

    // when
    let (ignored_games, regular_games) =
        trim_ignored_games(games, &ignored_releases, &ignored_flags, false);

    // then
    assert_eq!(ignored_games.len(), 3);
    assert_eq!(regular_games.len(), 1);
    assert_eq!(regular_games.get(0).unwrap().name, "Game (USA)")
}
