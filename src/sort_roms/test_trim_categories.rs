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
            device: false,
            bios: false,
            jbfolder: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            completion: 2,
            system_id: 1,
            parent_id: None,
            bios_id: None,
            playlist_id: None,
            categories: Some(String::from("Games")),
        },
        Game {
            id: 2,
            name: String::from("Game (USA) (Demo)"),
            description: String::from(""),
            comment: None,
            external_id: None,
            device: false,
            bios: false,
            jbfolder: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            completion: 2,
            system_id: 1,
            parent_id: None,
            bios_id: None,
            playlist_id: None,
            categories: Some(String::from("Demos")),
        },
        Game {
            id: 3,
            name: String::from("Game (USA) (Beta)"),
            description: String::from(""),
            comment: None,
            external_id: None,
            device: false,
            bios: false,
            jbfolder: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            completion: 2,
            system_id: 1,
            parent_id: None,
            bios_id: None,
            playlist_id: None,
            categories: Some(String::from("Games|Preproduction")),
        },
        Game {
            id: 4,
            name: String::from("Game (Japan)"),
            description: String::from(""),
            comment: None,
            external_id: None,
            device: false,
            bios: false,
            jbfolder: false,
            regions: String::from(""),
            sorting: Sorting::AllRegions as i64,
            completion: 2,
            system_id: 1,
            parent_id: None,
            bios_id: None,
            playlist_id: None,
            categories: None,
        },
    ];

    // when
    let (ignored_games, regular_games) =
        trim_ignored_games(games, &[], &[], &[], &["Demos", "Preproduction"], false);

    // then
    assert_eq!(
        ignored_games
            .iter()
            .map(|game| game.id)
            .collect::<Vec<i64>>(),
        vec![2, 3]
    );
    assert_eq!(
        regular_games
            .iter()
            .map(|game| game.id)
            .collect::<Vec<i64>>(),
        vec![1, 4]
    );
}
