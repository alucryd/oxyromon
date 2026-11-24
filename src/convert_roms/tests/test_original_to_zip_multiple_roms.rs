use super::*;
use crate::archive::ArchiveCompression;
use crate::test_support::TestEnv;
use std::path::PathBuf;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20230527) (PSN).dat").await;

    let mut romfile_paths: Vec<PathBuf> = vec![];
    let romfile_path = env.fixture("UP0001-BLUS00001.pkg").await;
    romfile_paths.push(romfile_path);
    let romfile_path = env.fixture("prfgmHWxGNxsfJ.rap").await;
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
    let game = find_game_by_id(&mut env.connection, roms[0].game_id).await;
    let mut games_by_id: HashMap<i64, Game> = HashMap::new();
    games_by_id.insert(roms[0].game_id, game);
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);

    // when
    to_archive(
        &mut env.connection,
        &env.progress_bar,
        &system,
        games_by_id,
        roms_by_game_id,
        romfiles_by_id,
        archive::ArchiveType::Zip,
        false,
        false,
        true,
        &ArchiveCompression::Default,
        false,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 2);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);

    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        env.stored_path(&system_directory.join("Test Game (USA, Europe) (DLC).zip")),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());

    let archive_romfiles = romfile
        .as_common(&mut env.connection)
        .await
        .unwrap()
        .as_archive(&env.progress_bar, None)
        .await
        .unwrap();
    assert_eq!(archive_romfiles.len(), 2);

    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "UP0001-BLUS00001.pkg");
    assert_eq!(rom.romfile_id, Some(romfile.id));

    let rom = roms.get(1).unwrap();
    assert_eq!(rom.name, "prfgmHWxGNxsfJ.rap");
    assert_eq!(rom.romfile_id, Some(romfile.id));
}
