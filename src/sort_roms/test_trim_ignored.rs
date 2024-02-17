use super::*;

#[tokio::test]
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
        Game {
            id: 5,
            name: String::from("Game (Europe) (De,Nl)"),
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
            id: 6,
            name: String::from("Game (Europe) (En,Fr)"),
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
        trim_ignored_games(games, &["En"], &ignored_releases, &ignored_flags, false);

    // then
    assert_eq!(ignored_games.len(), 4);
    assert_eq!(regular_games.len(), 2);
    assert_eq!(regular_games.first().unwrap().name, "Game (USA)");
    assert_eq!(regular_games.get(1).unwrap().name, "Game (Europe) (En,Fr)");
}
