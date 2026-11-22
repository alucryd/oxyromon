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

    // the same DAT again, forced, with a new name but no extension

    // when
    import_dat(
        &mut env.connection,
        &env.progress_bar,
        &datfile_xml,
        &detector_xml,
        Some(&String::from("Renamed Test System")),
        None,
        true,
        false,
    )
    .await
    .unwrap();

    // then
    let systems = find_systems(&mut env.connection).await;
    assert_eq!(systems.len(), 1);
    let system = systems.first().unwrap();
    assert_eq!(system.version, "20200721");
    assert_eq!(system.custom_name.as_deref(), Some("Renamed Test System"));
    assert!(system.custom_extension.is_none());
}
