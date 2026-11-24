use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721) (Parent-Clone).dat").await;

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
        false,
        false,
        false,
        &[],
    )
    .await
    .unwrap();

    // then
    let games = find_games_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(6, games.len());

    let one_regions_indices = vec![0, 1, 4];
    let trash_indices = vec![2, 3, 5];

    for i in one_regions_indices {
        assert_eq!(games[i].sorting, Sorting::OneRegion as i64);
    }

    for i in trash_indices {
        assert_eq!(games[i].sorting, Sorting::Ignored as i64);
    }
}
