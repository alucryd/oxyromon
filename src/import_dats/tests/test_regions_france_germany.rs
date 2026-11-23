use super::*;

#[test]
fn test() {
    // given
    let game_name = "Test Game (France, Germany)";

    // when
    let regions = get_regions_from_game_name(game_name).unwrap();

    // then
    assert_eq!(regions, "FR-DE");
}
