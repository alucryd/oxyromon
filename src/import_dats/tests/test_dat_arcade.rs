use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let dat_path = env.fixture_source("Test System (20200721).dat");
    let (datfile_xml, detector_xml) = parse_dat(&env.progress_bar, &dat_path, false)
        .await
        .unwrap();
    let newer_dat_path = env.fixture_source("Test System (20200722) (Renamed Game).dat");
    let (newer_datfile_xml, newer_detector_xml) =
        parse_dat(&env.progress_bar, &newer_dat_path, false)
            .await
            .unwrap();

    // when: a DAT listing games, imported as arcade
    import_dat(
        &mut env.connection,
        &env.progress_bar,
        &datfile_xml,
        &detector_xml,
        None,
        None,
        false,
        true,
    )
    .await
    .unwrap();

    // then: arcade, so no regions are parsed from the game names
    let system = env.system().await;
    assert!(system.arcade);
    let games = find_games(&mut env.connection).await;
    assert_eq!(games.len(), 6);
    assert!(games.iter().all(|game| game.regions.is_empty()));

    // when: an update without the flag
    import_dat(
        &mut env.connection,
        &env.progress_bar,
        &newer_datfile_xml,
        &newer_detector_xml,
        None,
        None,
        false,
        false,
    )
    .await
    .unwrap();

    // then: the system stays arcade
    assert!(env.system().await.arcade);

    // when: a forced import without the flag
    import_dat(
        &mut env.connection,
        &env.progress_bar,
        &newer_datfile_xml,
        &newer_detector_xml,
        None,
        None,
        true,
        false,
    )
    .await
    .unwrap();

    // then: the flag applies as given, and regions are back
    assert!(!env.system().await.arcade);
    assert!(
        find_games(&mut env.connection)
            .await
            .iter()
            .any(|game| !game.regions.is_empty())
    );
}
