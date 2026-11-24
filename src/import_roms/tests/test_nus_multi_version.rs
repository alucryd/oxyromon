use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    // the entry also lists an older version's TMD and a content only it uses
    env.import_dats(&["Test System (20261025) (CDN History).dat"])
        .await;

    // the newest version, whole
    let _nus_directory = env.fixture("Test Game (USA) (Update)").await;

    let system = env.system().await;

    // when
    let import_dir = env.tmp_directory.clone();
    env.run(&["import-roms", import_dir.to_str().unwrap()])
        .await
        .unwrap();

    // then the older TMD and the content only it uses are ignored
    let games = find_games_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(games[0].completion, Completion::Full as i64);
}
