use crate::database::*;
use super::{import_dat, parse_dat};
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;

    // first import the system from a plain DAT so it exists in the database
    let dat_path = env.fixture_source("Test System (20200721).dat");
    let (datfile_xml, detector_xml) = parse_dat(&env.progress_bar, &dat_path, false)
        .await
        .unwrap();

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

    let systems = find_systems(&mut env.connection).await;
    assert_eq!(systems.len(), 1);
    assert_eq!(systems.first().unwrap().version, "20200721");
    assert_eq!(find_games(&mut env.connection).await.len(), 6);
    assert_eq!(find_roms(&mut env.connection).await.len(), 8);

    // when: import the ZIP with --update flag; the system is known so it should be updated
    let zip_path = env.fixture_source("Test System (20210401).zip");
    env.run(&["import-dats", "-u", zip_path.to_str().unwrap()])
        .await
        .unwrap();

    // then: the system should have been updated to the newer version
    let systems = find_systems(&mut env.connection).await;
    assert_eq!(systems.len(), 1);

    let system = systems.first().unwrap();
    assert_eq!(system.name, "Test System");
    assert_eq!(system.version, "20210401");

    assert_eq!(find_games(&mut env.connection).await.len(), 3);
    assert_eq!(find_roms(&mut env.connection).await.len(), 3);
}
