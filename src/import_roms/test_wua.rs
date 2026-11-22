use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261025) (WUD).dat").await;

    // a lossless WUA, its update partition in the store beside it
    let staging = env.tmp_directory.join("staging");
    fs::create_dir(&staging).await.unwrap();
    let wua_path = staging.join("Test Game (USA).wua");
    let staging_store = staging.join(wux_rs::STORE_DIR);
    let keys = wux_rs::load_keys(&env.fixture_source("Test Game (USA).keys.txt")).unwrap();
    wux_rs::to_wua(
        &[&env.fixture_source("Test Game (USA).wux")],
        &wua_path,
        &keys,
        Some(&staging_store),
        &mut |_| {},
    )
    .unwrap();
    let entries = wux_rs::store_entries(&wua_path).unwrap();
    assert_eq!(entries.len(), 1);

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();

    // when
    env.import_roms(&[&staging]).await;

    // then the WUA is matched by the disc it holds, and its update partition
    // copied into the system's store, the staging store left as it was
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 1);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA).iso");
    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA).wua")
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert_eq!(rom.romfile_id, Some(romfile.id));
    assert!(
        system_directory
            .join(wux_rs::STORE_DIR)
            .join(&entries[0])
            .is_file()
    );
    assert!(staging_store.join(&entries[0]).is_file());
}
