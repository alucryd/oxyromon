use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261025) (WUD).dat").await;

    let romfile_path = env.fixture("Test Game (USA).wux").await;

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    env.import_roms(&[&romfile_path]).await;

    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    let romfile = find_romfile_by_id(&mut env.connection, roms[0].romfile_id.unwrap()).await;
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    romfiles_by_id.insert(romfile.id, romfile);

    // when
    to_wua(
        &mut env.connection,
        &env.progress_bar,
        &system,
        roms_by_game_id,
        romfiles_by_id,
        false,
        true,
    )
    .await
    .unwrap();

    // then the WUA rebuilds the disc, its update partition in the store
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        env.stored_path(&system_directory.join("Test Game (USA).wua")),
    );
    assert!(!system_directory.join("Test Game (USA).wux").exists());
    let wua = env.rom_directory.join(&romfile.path);
    let entries = wux_rs::store_entries(&wua).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(
        system_directory
            .join(wux_rs::STORE_DIR)
            .join(&entries[0])
            .is_file()
    );
    // and it weighs its file and the partition, which only it needs
    let partition = system_directory.join(wux_rs::STORE_DIR).join(&entries[0]);
    assert_eq!(
        crate::ore::actual_size(&mut env.connection, &romfiles)
            .await
            .unwrap(),
        romfile.size as u64 + partition.metadata().unwrap().len()
    );
    // and half of it once another WUA needs it too
    let copy = system_directory.join("Test Game (Copy).wua");
    std::fs::copy(&wua, &copy).unwrap();
    create_romfile(
        &mut env.connection,
        copy.strip_prefix(&env.rom_directory)
            .unwrap()
            .to_str()
            .unwrap(),
        romfile.size as u64,
        RomfileType::Romfile,
    )
    .await;
    assert_eq!(
        crate::ore::actual_size(&mut env.connection, &romfiles)
            .await
            .unwrap(),
        romfile.size as u64 + partition.metadata().unwrap().len() / 2
    );
    std::fs::remove_file(&copy).unwrap();
    let info = wux_rs::disc_info(&wua).unwrap().unwrap();
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(Some(hex(&info.sha1)), roms[0].sha1);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
