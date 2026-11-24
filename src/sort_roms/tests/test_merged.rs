use super::*;
use crate::rebuild_roms::merged_set::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;

    import_split_set(&mut env).await;
    let system = rebuild(&mut env.connection, &env.progress_bar, "MERGED").await;

    // when
    let matches = subcommand().get_matches_from(["sort-roms", "-a", "-y"]);
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();

    // then the clones do not drag their parent's ZIP to their own names
    assert_merged(&mut env.connection, &system).await;
}
