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
    let nus_directory = env.tmp_directory.join("Test Game (USA) (Update)");
    std::fs::create_dir(&nus_directory).unwrap();
    for entry in std::fs::read_dir(env.fixture_source("Test Game (USA) (Update)")).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, nus_directory.join(path.file_name().unwrap())).unwrap();
    }

    let system = env.system().await;

    // when
    let env_tmp_directory_1 = env.tmp_directory.clone();
    env.run(&["import-roms", env_tmp_directory_1.to_str().unwrap()])
        .await
        .unwrap();

    // then the older TMD and the content only it uses are ignored
    let games = find_games_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(games[0].completion, Completion::Full as i64);
}
