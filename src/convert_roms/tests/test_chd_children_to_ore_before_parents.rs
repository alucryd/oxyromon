use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

/// The only byte the clone's disc differs from its parent's, in a run of
/// padding, so that the clone's CHD reads every other hunk from the parent's.
const CLONE_OFFSET: usize = 0xc0000;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261115) (CHD Clone).dat").await;

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    // a disc and a clone of it, differing by a byte of padding
    let iso_path = env.fixture("Test Game (USA).iso").await;
    let mut clone = fs::read(&iso_path).await.unwrap();
    clone[CLONE_OFFSET] = 0xff;
    let clone_path = env.tmp_directory.join("Test Game (USA) (Rev 1).iso");
    fs::write(&clone_path, &clone).await.unwrap();

    // a CHD of each, the clone's reading from the parent's
    let parent_chd = CommonRomfile::from_path(&iso_path)
        .unwrap()
        .as_iso()
        .unwrap()
        .to_chd(&env.progress_bar, &env.tmp_directory, &[], &None, None)
        .await
        .unwrap()
        .romfile;
    let clone_chd = CommonRomfile::from_path(&clone_path)
        .unwrap()
        .as_iso()
        .unwrap()
        .to_chd(
            &env.progress_bar,
            &env.tmp_directory,
            &[],
            &None,
            Some(parent_chd.clone()),
        )
        .await
        .unwrap()
        .romfile;

    // import them: only the CHDs themselves keep the clone and its parent together
    env.run(&[
        "import-roms",
        parent_chd.path.as_os_str().to_str().unwrap(),
        clone_chd.path.as_os_str().to_str().unwrap(),
    ])
    .await
    .unwrap();

    let games = find_full_games_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(games.len(), 2);
    // the parent before the clone, the order the games are stored in
    assert_eq!(games.first().unwrap().name, "Test Game (USA)");
    let romfiles = find_romfiles(&mut env.connection).await;
    assert!(romfiles.iter().all(|romfile| romfile.parent_id.is_none()));

    // the clone is nothing without its parent
    let orphan_path = env.tmp_directory.join("orphan.chd");
    fs::copy(
        system_directory.join("Test Game (USA) (Rev 1).chd"),
        &orphan_path,
    )
    .await
    .unwrap();
    let mut orphan = chd_rs::Chd::open(&orphan_path).unwrap();
    assert!(orphan.read_bytes(0x8000, &mut [0u8; 2048]).is_err());

    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    for game in &games {
        let roms = find_roms_with_romfile_by_game_ids(&mut env.connection, &[game.id]).await;
        for rom in &roms {
            let romfile = find_romfile_by_id(&mut env.connection, rom.romfile_id.unwrap()).await;
            romfiles_by_id.insert(romfile.id, romfile);
        }
        roms_by_game_id.insert(game.id, roms);
    }

    // when
    to_ore(
        &mut env.connection,
        &env.progress_bar,
        roms_by_game_id,
        romfiles_by_id,
        false,
        false,
        true,
        &OreCompressionAlgorithm::Zstd,
        &OreProfile::Stream,
        None,
        None,
    )
    .await
    .unwrap();

    // then
    // the clone converted first, and its parent after it, both into complete OREs
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 2);
    for romfile in &romfiles {
        assert!(romfile.path.ends_with(".ore"), "{}", romfile.path);
        assert!(env.rom_directory.join(&romfile.path).is_file());
    }
    for name in ["Test Game (USA)", "Test Game (USA) (Rev 1)"] {
        assert!(!system_directory.join(format!("{name}.chd")).exists());
    }
}
