use super::*;

#[async_std::test]
async fn test() {
    // given
    let game_a = Game {
        id: 1,
        name: String::from("Game (USA)"),
        description: String::from(""),
        comment: None,
        external_id: None,
        bios: false,
        jbfolder: false,
        regions: String::from("US"),
        sorting: Sorting::AllRegions as i64,
        complete: true,
        system_id: 1,
        parent_id: Some(1),
        bios_id: None,
        playlist_id: None,
    };
    let game_b = Game {
        id: 1,
        name: String::from("Game (Europe)"),
        description: String::from(""),
        comment: None,
        external_id: None,
        bios: false,
        jbfolder: false,
        regions: String::from("EU"),
        sorting: Sorting::AllRegions as i64,
        complete: true,
        system_id: 1,
        parent_id: Some(1),
        bios_id: None,
        playlist_id: None,
    };

    // when
    let ordering = sort_games_by_weight(
        &game_a,
        &game_b,
        true,
        &PreferredRegion::None,
        &PreferredVersion::None,
        &[],
    );

    // then
    assert_eq!(ordering, Ordering::Equal);
}
