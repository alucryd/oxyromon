use super::*;
use crate::test_support::TestEnv;
use crate::xso::XsoType;

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
    to_xso(
        &mut env.connection,
        &env.progress_bar,
        roms_by_game_id,
        romfiles_by_id,
        false,
        true,
        true,
        XsoType::Zso,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 3);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 2);

    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        env.stored_path(&system_directory.join("Test Game (USA, Europe) (CUE BIN).chd")),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());

    let romfile = romfiles.get(1).unwrap();
    assert_eq!(
        romfile.path,
        env.stored_path(&system_directory.join("Test Game (USA, Europe) (CUE BIN).cue")),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
}
