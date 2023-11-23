use super::*;

#[tokio::test]
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
        name: String::from("Game (USA) (Rumble Version)"),
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

    // when
    let ordering = sort_games_by_weight(
        &game_a,
        &game_b,
        false,
        &PreferredRegion::None,
        &PreferredVersion::None,
        &["Rumble Version"],
    );

    // then
    assert_eq!(ordering, Ordering::Greater);
}
