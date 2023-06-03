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
        parent_id: Some(3),
        bios_id: None,
        playlist_id: None,
    };
    let game_b = Game {
        id: 1,
        name: String::from("Game (World)"),
        description: String::from(""),
        comment: None,
        external_id: None,
        bios: false,
        jbfolder: false,
        regions: String::from("US-EU-JP"),
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
        &PreferredRegion::Narrow,
        &PreferredVersion::None,
        &[],
    );

    // then
    assert_eq!(ordering, Ordering::Less);
}
