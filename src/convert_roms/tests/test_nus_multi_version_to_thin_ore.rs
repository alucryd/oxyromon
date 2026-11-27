use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given a title whose entry also lists an older version's TMD and a
    // content only it uses, its newest version imported whole
    let mut env = TestEnv::new().await;
    env.import_dats(&["Test System (20261025) (CDN History).dat"])
        .await;
    env.fixture("Test Game (USA) (Update)").await;
    let import_dir = env.tmp_directory.clone();
    env.run(&["import-roms", import_dir.to_str().unwrap()])
        .await
        .unwrap();
    let system = env.system().await;
    env.set_bool("ORE_STORE", true, system.id).await;

    // when converted to a thin ORE, and its completion computed again
    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();
    compute_system_completion(&mut env.connection, &env.progress_bar, &system)
        .await
        .unwrap();

    // then it is still complete, its newest TMD read through the store
    let romfiles = env.romfiles().await;
    assert!(
        romfiles
            .iter()
            .all(|romfile| romfile.path.ends_with(".ore"))
    );
    assert!(
        env.system_directory(&system)
            .await
            .join(".ore-store")
            .is_dir()
    );
    let games = find_games_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(games[0].completion, Completion::Full as i64);
}
