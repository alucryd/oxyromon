use super::merged_set::*;
use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    import_split_set(&mut env).await;
    let system = rebuild(&mut env.connection, &env.progress_bar, "NON_MERGED").await;
    assert_eq!(system.merging, Merging::NonMerged as i64);

    // when
    let system = rebuild(&mut env.connection, &env.progress_bar, "MERGED").await;

    // then the clones' copies of their parent's files are gone with their ZIPs
    assert_merged(&mut env.connection, &system).await;
    assert_checks(&mut env.connection, &env.progress_bar).await;
}
