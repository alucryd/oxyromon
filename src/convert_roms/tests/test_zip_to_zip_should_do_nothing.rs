use super::*;
use crate::archive::ArchiveCompression;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe).rom.zip").await;

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    env.import_roms(&[&romfile_path]).await;

    let games = find_full_games_by_system_id(&mut env.connection, system.id).await;
    let roms = find_roms_with_romfile_by_game_ids(&mut env.connection, &[games[0].id]).await;
    let romfile = find_romfile_by_id(&mut env.connection, roms[0].romfile_id.unwrap()).await;
    let old_mtime = fs::metadata(&romfile.as_common(&mut env.connection).await.unwrap().path)
        .await
        .unwrap()
        .modified()
        .unwrap();
    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    roms_by_game_id.insert(roms[0].game_id, roms);
    let games_by_id: HashMap<i64, Game> = games.into_iter().map(|game| (game.id, game)).collect();
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    romfiles_by_id.insert(romfile.id, romfile);

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
    assert_eq!(roms.len(), 1);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);

    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA, Europe).rom");

    let romfile = romfiles.first().unwrap();
    let new_mtime = fs::metadata(&romfile.as_common(&mut env.connection).await.unwrap().path)
        .await
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        romfile.path,
        env.stored_path(&system_directory.join("Test Game (USA, Europe).zip")),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));
    assert_eq!(old_mtime, new_mtime);
}
