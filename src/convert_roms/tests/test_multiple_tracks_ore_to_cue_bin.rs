use super::*;
use crate::test_support::TestEnv;
use std::path::PathBuf;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let mut romfile_paths: Vec<PathBuf> = vec![];
    let romfile_path = env
        .fixture("Test Game (USA, Europe) (Multiple Tracks).cue")
        .await;
    romfile_paths.push(romfile_path);
    let romfile_path = env
        .fixture("Test Game (USA, Europe) (CUE BIN) (Track 01).bin")
        .await;
    romfile_paths.push(romfile_path);
    let romfile_path = env
        .fixture("Test Game (USA, Europe) (CUE BIN) (Track 02).bin")
        .await;
    romfile_paths.push(romfile_path);

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    for romfile_path in romfile_paths {
        env.import_roms(&[&romfile_path]).await;
    }

    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    for rom in &roms {
        let romfile = find_romfile_by_id(&mut env.connection, rom.romfile_id.unwrap()).await;
        romfiles_by_id.insert(romfile.id, romfile);
    }
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);

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

    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    for rom in &roms {
        let romfile = find_romfile_by_id(&mut env.connection, rom.romfile_id.unwrap()).await;
        romfiles_by_id.insert(romfile.id, romfile);
    }
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);

    // when
    to_original(
        &mut env.connection,
        &env.progress_bar,
        &system,
        roms_by_game_id,
        romfiles_by_id,
        true,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 3);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 3);

    // each ROM its own file again, the cue sheet included
    for rom in &roms {
        let romfile = find_romfile_by_id(&mut env.connection, rom.romfile_id.unwrap()).await;
        assert_eq!(
            romfile.path,
            system_directory
                .join(&rom.name)
                .strip_prefix(&env.rom_directory)
                .unwrap()
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(env.rom_directory.join(&romfile.path).is_file());
    }
    assert!(
        !system_directory
            .join("Test Game (USA, Europe) (CUE BIN).ore")
            .exists()
    );
}
