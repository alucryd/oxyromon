use super::*;

#[test]
fn test() {
    // given
    let game = Game {
        id: 1,
        name: String::from("Test Game (USA, Europe)"),
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
    let games = prompt_for_games(vec![game], false).unwrap();

    // then
    assert_eq!(games.len(), 1);
    assert_eq!(games[0].name, "Test Game (USA, Europe)");
}
