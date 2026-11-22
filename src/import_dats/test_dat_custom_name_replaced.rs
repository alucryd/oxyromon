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
    import_dat(
        &mut env.connection,
        &env.progress_bar,
        &datfile_xml,
        &detector_xml,
        Some(&String::from("Custom Test System")),
        Some(&String::from("bin")),
        false,
        false,
    )
    .await
    .unwrap();

    // a newer DAT for the same system, with a new name but no extension
    let dat_path = env.fixture_source("Test System (20210401).dat");
    let (datfile_xml, detector_xml) = parse_dat(&env.progress_bar, &dat_path, false)
        .await
        .unwrap();

    // when
    import_dat(
        &mut env.connection,
        &env.progress_bar,
        &datfile_xml,
        &detector_xml,
        Some(&String::from("Renamed Test System")),
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
    assert_eq!(system.version, "20210401");
    assert_eq!(system.custom_name.as_deref(), Some("Renamed Test System"));
    assert_eq!(system.custom_extension.as_deref(), Some("bin"));
}
