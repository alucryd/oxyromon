use super::*;

#[tokio::test]
async fn test() {
    // given
    let game_a = Game {
        id: 1,
        name: String::from("Game (USA) (Rev 2)"),
        description: String::from(""),
        comment: None,
        external_id: None,
        device: false,
        bios: false,
        jbfolder: false,
        regions: String::from(""),
        sorting: Sorting::AllRegions as i64,
        complete: true,
        system_id: 1,
        parent_id: Some(3),
        bios_id: None,
        playlist_id: None,
    };
    let game_b = Game {
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
        complete: true,
        system_id: 1,
        parent_id: Some(3),
        bios_id: None,
        playlist_id: None,
    };

    // when
    let ordering = sort_games_by_weight(
        &game_a,
        &game_b,
        false,
        &PreferredRegion::None,
        &PreferredVersion::Old,
        &[],
    );

    // then
    assert_eq!(ordering, Ordering::Greater);
}
