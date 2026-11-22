use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20240908) (Patches).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe).rom").await;

    let patch_path = env.fixture("Test Game (USA, Europe).ips").await;

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();

    env.import_roms(&[&romfile_path]).await;

    // when
    import_patch(
        &mut env.connection,
        &env.progress_bar,
        &patch_path,
        &PatchType::Ips,
        false,
        false,
        None,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 1);
    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe).rom");
    let patches = find_patches_by_rom_id(&mut env.connection, rom.id).await;
    assert_eq!(patches.len(), 1);
    let patch = patches.first().unwrap();
    assert_eq!(patch.name, "Test Game (USA, Europe)");
    assert_eq!(patch.index, 0);
    assert_eq!(patch.rom_id, rom.id);

    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 2);
    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe).ips")
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
    assert_eq!(patch.romfile_id, romfile.id);
}
