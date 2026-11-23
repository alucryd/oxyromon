use super::*;
use crate::database::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    let mut env = TestEnv::new().await;
    // given a base system and a subsystem grouped in its directory, their
    // store settings set apart
    env.import_dats(&[
        "Test System (20200721).dat",
        "Test System (20261025) (CDN).dat",
    ])
    .await;
    let systems = find_systems(&mut env.connection).await;
    let base = systems.iter().find(|s| s.name == "Test System").unwrap();
    let cdn = systems
        .iter()
        .find(|s| s.name == "Test System (CDN)")
        .unwrap();
    set_bool(&mut env.connection, "ORE_STORE", true, Some(base.id)).await;
    set_bool(&mut env.connection, "ORE_STORE", false, Some(cdn.id)).await;
    set_integer(&mut env.connection, "ORE_STORE_PARITY", 4, Some(base.id)).await;
    set_integer(&mut env.connection, "ORE_STORE_PARITY", 12, Some(cdn.id)).await;

    // then both follow the base system, whose directory they share
    assert_eq!(
        get_system_directory(&mut env.connection, cdn)
            .await
            .unwrap(),
        get_system_directory(&mut env.connection, base)
            .await
            .unwrap()
    );
    assert_eq!(
        store_settings_system_id(&mut env.connection, base)
            .await
            .unwrap(),
        base.id
    );
    assert_eq!(
        store_settings_system_id(&mut env.connection, cdn)
            .await
            .unwrap(),
        base.id
    );
    let sink = StoreSink::open(&mut env.connection, cdn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(sink.parity, 4);

    // and without grouping, a subsystem's settings are its own again
    set_bool(&mut env.connection, "GROUP_SUBSYSTEMS", false, Some(cdn.id)).await;
    assert_eq!(
        store_settings_system_id(&mut env.connection, cdn)
            .await
            .unwrap(),
        cdn.id
    );
    assert!(
        StoreSink::open(&mut env.connection, cdn)
            .await
            .unwrap()
            .is_none()
    );
}
