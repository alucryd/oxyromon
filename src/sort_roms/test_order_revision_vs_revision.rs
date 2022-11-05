use super::*;

#[async_std::test]
async fn test() {
    // given
    let game_a = Game {
        id: 1,
        name: String::from("Game (USA) (Rev 1)"),
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
    };
    let game_b = Game {
        id: 1,
        name: String::from("Game (USA) (Rev 2)"),
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
    };

    // when
    let ordering = sort_games_by_version_or_name_desc(&game_a, &game_b);

    // then
    assert_eq!(ordering, Ordering::Greater);
}
