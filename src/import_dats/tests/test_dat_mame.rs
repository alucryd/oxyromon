use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let dat_path = env.fixture_source("Test System (20200721) (MAME).dat");
    let (datfile_xml, detector_xml) = parse_dat(&env.progress_bar, &dat_path, false)
        .await
        .unwrap();

    // when
    import_dat(
        &mut env.connection,
        &env.progress_bar,
        &datfile_xml,
        &detector_xml,
        None,
        None,
        false,
        false,
    )
    .await
    .unwrap();

    // then
    let systems = find_systems(&mut env.connection).await;
    assert_eq!(systems.len(), 1);

    let system = systems.first().unwrap();
    assert_eq!(system.name, "Test System (MAME)");
    assert!(system.arcade);

    assert_eq!(find_games(&mut env.connection).await.len(), 1);
    assert_eq!(find_roms(&mut env.connection).await.len(), 1);
}
