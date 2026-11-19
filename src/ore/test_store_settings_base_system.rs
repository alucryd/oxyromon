use super::super::database::*;
use super::super::import_dats;
use super::*;
use std::path::PathBuf;
use tempfile::TempDir;

#[tokio::test]
async fn test() {
    // given a base system and a subsystem grouped in its directory, their
    // store settings set apart
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = crate::config::TestDatabase::new();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    for dat in [
        "tests/Test System (20200721).dat",
        "tests/Test System (20261025) (CDN).dat",
    ] {
        let matches = import_dats::subcommand().get_matches_from(["import-dats", dat]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();
    }
    let systems = find_systems(&mut connection).await;
    let base = systems.iter().find(|s| s.name == "Test System").unwrap();
    let cdn = systems
        .iter()
        .find(|s| s.name == "Test System (CDN)")
        .unwrap();
    set_bool(&mut connection, "ORE_STORE", true, Some(base.id)).await;
    set_bool(&mut connection, "ORE_STORE", false, Some(cdn.id)).await;
    set_integer(&mut connection, "ORE_STORE_PARITY", 4, Some(base.id)).await;
    set_integer(&mut connection, "ORE_STORE_PARITY", 12, Some(cdn.id)).await;

    // then both follow the base system, whose directory they share
    assert_eq!(
        get_system_directory(&mut connection, cdn).await.unwrap(),
        get_system_directory(&mut connection, base).await.unwrap()
    );
    assert_eq!(
        store_settings_system_id(&mut connection, base)
            .await
            .unwrap(),
        base.id
    );
    assert_eq!(
        store_settings_system_id(&mut connection, cdn)
            .await
            .unwrap(),
        base.id
    );
    let sink = StoreSink::open(&mut connection, cdn)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(sink.parity, 4);

    // and without grouping, a subsystem's settings are its own again
    set_bool(&mut connection, "GROUP_SUBSYSTEMS", false, Some(cdn.id)).await;
    assert_eq!(
        store_settings_system_id(&mut connection, cdn)
            .await
            .unwrap(),
        cdn.id
    );
    assert!(
        StoreSink::open(&mut connection, cdn)
            .await
            .unwrap()
            .is_none()
    );
}
