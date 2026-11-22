use super::super::database::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;

    // the ZIP contains: Test System/Test System (20210401).dat (nested in a subdirectory)
    let zip_path = env.fixture_source("Test System (20210401).zip");

    // when
    env.run(&["import-dats", zip_path.to_str().unwrap()])
        .await
        .unwrap();

    // then
    let systems = find_systems(&mut env.connection).await;
    assert_eq!(systems.len(), 1);

    let system = systems.first().unwrap();
    assert_eq!(system.name, "Test System");
    assert_eq!(system.version, "20210401");

    assert_eq!(find_games(&mut env.connection).await.len(), 3);
    assert_eq!(find_roms(&mut env.connection).await.len(), 3);
}
