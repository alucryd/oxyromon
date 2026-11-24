use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe).rom").await;

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    env.import_roms(&[&romfile_path]).await;

    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    let romfile = find_romfile_by_id(&mut env.connection, roms[0].romfile_id.unwrap()).await;
    let mut roms_by_game_id: HashMap<i64, Vec<Rom>> = HashMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    romfiles_by_id.insert(romfile.id, romfile);

    let matches =
        subcommand().get_matches_from(["convert-roms", "-f", "ZIP", "-g", "%test gqme%", "-a"]);

    // when
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 1);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);

    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe).rom");

    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        env.stored_path(&system_directory.join("Test Game (USA, Europe).rom")),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));
}
