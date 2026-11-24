use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261110) (Infer Clones).dat").await;

    let system = env.system().await;

    let all_regions = vec![];
    let one_regions = vec![Region::UnitedStates, Region::Europe];

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
    assert_eq!(7, games.len());

    let sortings: Vec<(&str, i64)> = games
        .iter()
        .map(|game| (game.name.as_str(), game.sorting))
        .collect();
    assert_eq!(
        sortings,
        vec![
            ("Another Game (USA)", Sorting::OneRegion as i64),
            ("Another Game (USA) (Premium Box)", Sorting::Ignored as i64),
            (
                "Another Game (USA) (Premium Pack Bonus Disc)",
                Sorting::OneRegion as i64
            ),
            ("Test Game (Europe) (Disc 1)", Sorting::Ignored as i64),
            ("Test Game (Europe) (Disc 2)", Sorting::Ignored as i64),
            ("Test Game (USA) (Disc 1)", Sorting::OneRegion as i64),
            ("Test Game (USA) (Disc 2)", Sorting::OneRegion as i64),
        ]
    );
}
