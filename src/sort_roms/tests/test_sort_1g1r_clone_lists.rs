use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261111) (Clone Lists).dat").await;

    env.run(&[
        "import-clone-lists",
        env.fixture_source("Test System (Clone Lists) (Redump).json")
            .to_str()
            .unwrap(),
    ])
    .await
    .unwrap();

    let system = env.system().await;

    let all_regions = vec![];
    let one_regions = vec![
        Region::UnitedStates,
        Region::Japan,
        Region::Germany,
        Region::Spain,
    ];

    // when
    sort_system(
        &mut env.connection,
        &env.progress_bar,
        true,
        false,
        &system,
        &all_regions,
        &one_regions,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        true,
        &PreferredRegion::None,
        &PreferredVersion::None,
        &[],
        &None,
        &None,
        true,
        true,
        false,
        &[],
    )
    .await
    .unwrap();

    // then
    let games = find_games_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(14, games.len());

    let sortings: Vec<(&str, i64)> = games
        .iter()
        .map(|game| (game.name.as_str(), game.sorting))
        .collect();
    assert_eq!(
        sortings,
        vec![
            // the list prefers the Premium Box within its region
            ("Another Game (Japan)", Sorting::Ignored as i64),
            (
                "Another Game (Japan) (Premium Box)",
                Sorting::OneRegion as i64
            ),
            // the first game is a group of its own in Japan, where the sequel
            // is the first game elsewhere
            ("First Game (Japan)", Sorting::OneRegion as i64),
            ("First Game (USA)", Sorting::OneRegion as i64),
            // the list's title naming the Doukon Pack is more specific than
            // the one naming the game
            ("Fishing Game (Japan)", Sorting::OneRegion as i64),
            (
                "Fishing Game (Japan) (Doukon Pack)",
                Sorting::OneRegion as i64
            ),
            ("Sequel Game (Japan)", Sorting::Ignored as i64),
            ("Sequel Game (USA)", Sorting::OneRegion as i64),
            // and the Spanish and German releases are the same game under
            // other names, found by a regex and by their full names
            ("Test Game (USA)", Sorting::OneRegion as i64),
            ("Testjuego (Spain)", Sorting::Ignored as i64),
            ("Testspiel (Germany)", Sorting::Ignored as i64),
            // the English release and the revision are games of their own, by
            // their languages and by their names
            ("Word Game (Germany)", Sorting::OneRegion as i64),
            ("Word Game (Germany) (En,De)", Sorting::OneRegion as i64),
            ("Word Game (Germany) (Rev 1)", Sorting::OneRegion as i64),
        ]
    );
}
