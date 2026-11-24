use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;

    let key = "REGIONS_ONE";

    add_to_list(&mut env.connection, &env.progress_bar, key, "US", None).await;
    let matches = subcommand().get_matches_from(["sort-roms", "-y", "-o", "EU"]);

    // when
    let all_regions = get_regions(&mut env.connection, &matches, key, None).await;

    // then
    assert_eq!(all_regions.len(), 1);
    assert_eq!(all_regions.first().unwrap(), &Region::Europe);
}
