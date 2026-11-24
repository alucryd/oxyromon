//! Converting to a Zstandard compressed 7z.
//!
//! Zstandard is the default for both formats, and only the native
//! backend can read or write it: 7-Zip carries no Zstandard codec. The
//! round trip through `as_archive` is the point of the test — it proves
//! what was written can be listed and its entry checked afterwards.

use super::*;
use crate::archive::ArchiveCompression;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe).rom").await;

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    env.import_roms(&[&romfile_path]).await;

    let games = find_full_games_by_system_id(&mut env.connection, system.id).await;
    let roms =
        find_roms_with_romfile_by_game_ids(&mut env.connection, &[games.first().unwrap().id]).await;
    let romfile = find_romfile_by_id(&mut env.connection, roms[0].romfile_id.unwrap()).await;
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
        archive::ArchiveType::Sevenzip,
        false,
        false,
        true,
        &ArchiveCompression::Zstd(19),
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
    assert_eq!(
        romfile.path,
        env.stored_path(&system_directory.join("Test Game (USA, Europe).7z")),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
    assert_eq!(rom.romfile_id, Some(romfile.id));

    let archive_romfiles = romfile
        .as_common(&mut env.connection)
        .await
        .unwrap()
        .as_archive(&env.progress_bar, None)
        .await
        .unwrap();
    assert_eq!(archive_romfiles.len(), 1);
    let entry = archive_romfiles.first().unwrap();
    assert_eq!(entry.path, "Test Game (USA, Europe).rom");
    assert_eq!(entry.size, 256);
    assert_eq!(entry.crc, rom.crc.clone().unwrap());
}
