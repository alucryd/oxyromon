use super::*;

fn game(id: i64, name: &str) -> Game {
    Game {
        id,
        name: String::from(name),
        description: String::new(),
        comment: None,
        external_id: None,
        device: false,
        bios: false,
        jbfolder: false,
        regions: String::new(),
        sorting: Sorting::AllRegions as i64,
        completion: 2,
        system_id: 1,
        parent_id: None,
        bios_id: None,
        playlist_id: None,
    }
}

/// LANGUAGES discards on its own, without any DISCARD_* list set.
#[tokio::test]
async fn test() {
    // given
    let games = vec![
        game(1, "Game (Europe) (De,Nl)"),
        game(2, "Game (Europe) (En,Fr)"),
        game(3, "Game (USA) (Beta)"),
    ];

    // when
    let (ignored_games, regular_games) = trim_ignored_games(games, &["En"], &[], &[], false);

    // then
    assert_eq!(ignored_games.len(), 1);
    assert_eq!(ignored_games[0].name, "Game (Europe) (De,Nl)");
    assert_eq!(regular_games.len(), 2);
}
