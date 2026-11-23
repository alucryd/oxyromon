use crate::database::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;

    // no system has been imported yet; the database is empty
    let systems = find_systems(&mut env.connection).await;
    assert_eq!(systems.len(), 0);

    // when: import the ZIP with --update flag; the system is unknown so it should be skipped
    let zip_path = env.fixture_source("Test System (20210401).zip");
    env.run(&["import-dats", "-u", zip_path.to_str().unwrap()])
        .await
        .unwrap();

    // then: no system should have been created
    let systems = find_systems(&mut env.connection).await;
    assert_eq!(systems.len(), 0);
    assert_eq!(find_games(&mut env.connection).await.len(), 0);
    assert_eq!(find_roms(&mut env.connection).await.len(), 0);
}
