use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let cue_romfile_path = env
        .fixture("Test Game (USA, Europe) (Multiple Tracks).cue")
        .await;
    let chd_romfile_path = env
        .fixture("Test Game (USA, Europe) (Multiple Tracks).chd")
        .await;

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    env.run(&[
        "import-roms",
        chd_romfile_path.as_os_str().to_str().unwrap(),
        cue_romfile_path.as_os_str().to_str().unwrap(),
    ])
    .await
    .unwrap();

    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    for rom in &roms {
        let romfile = find_romfile_by_id(&mut env.connection, rom.romfile_id.unwrap()).await;
        romfiles_by_id.insert(romfile.id, romfile);
    }
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);

    // when
    to_ore(
        &mut env.connection,
        &env.progress_bar,
        roms_by_game_id,
        romfiles_by_id,
        false,
        true,
        true,
        &OreCompressionAlgorithm::Zstd,
        &OreProfile::Stream,
        None,
        None,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 3);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);

    // the CHD's tracks and the cue sheet beside it, one ORE
    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        env.stored_path(&system_directory.join("Test Game (USA, Europe) (CUE BIN).ore")),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
    assert!(roms.iter().all(|rom| rom.romfile_id == Some(romfile.id)));
    for name in [
        "Test Game (USA, Europe) (CUE BIN).chd",
        "Test Game (USA, Europe) (CUE BIN).cue",
    ] {
        assert!(!system_directory.join(name).exists());
    }
}
