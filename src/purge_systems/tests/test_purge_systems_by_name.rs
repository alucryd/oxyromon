use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    env.import_dats(&[
        "Test System (20200721).dat",
        "Test System (20230105) (Multiple Discs).dat",
    ])
    .await;

    let romfile_path = env.fixture("Test Game (USA, Europe).rom").await;
    env.run(&[
        "import-roms",
        "-s",
        "Test System",
        romfile_path.to_str().unwrap(),
    ])
    .await
    .unwrap();

    // when
    env.run(&["purge-systems", "--system", "Test System"])
        .await
        .unwrap();

    // then: only the system named exactly is purged, without a prompt
    let systems = find_systems(&mut env.connection).await;
    assert_eq!(systems.len(), 1);
    assert_eq!(systems[0].name, "Test System (Multiple Discs)");

    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    assert!(romfiles[0].path.contains("Trash"));
}
