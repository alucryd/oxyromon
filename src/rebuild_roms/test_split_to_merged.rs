use super::merged_set::*;
use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let system = import_split_set(&mut env.connection, &env.progress_bar, &env.tmp_directory).await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();

    // when rebuilding to MERGED
    let system = rebuild(&mut env.connection, &env.progress_bar, "MERGED").await;

    // then the parent's ZIP holds every file once, the clashing one under its clone
    assert_merged(&mut env.connection, &system).await;
    assert_checks(&mut env.connection, &env.progress_bar).await;

    // when rebuilding back to SPLIT
    let system = rebuild(&mut env.connection, &env.progress_bar, "SPLIT").await;

    // then each set has its own ZIP again
    assert_eq!(system.merging, Merging::Split as i64);
    assert_eq!(
        entry_names(&system_directory.join("Test Parent.zip")),
        ["Test Common.rom", "Test Program.rom"]
    );
    assert_eq!(
        entry_names(&system_directory.join("Test Clone 1.zip")),
        ["Test Clones.rom", "Test Program.rom"]
    );
    assert_eq!(
        entry_names(&system_directory.join("Test Clone 2.zip")),
        ["Test Clones.rom"]
    );
    assert_eq!(find_romfiles(&mut env.connection).await.len(), 3);
    for game in find_games_by_system_id(&mut env.connection, system.id).await {
        assert_eq!(game.completion, Completion::Full as i64, "{}", game.name);
    }
    assert_checks(&mut env.connection, &env.progress_bar).await;
}
