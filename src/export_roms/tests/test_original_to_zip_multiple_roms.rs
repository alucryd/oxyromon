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

    for romfile_path in romfile_paths {
        env.import_roms(&[&romfile_path]).await;
    }

    let mut roms_by_game_id: IndexMap<i64, Vec<Rom>> = IndexMap::new();
    let mut games_by_id: HashMap<i64, Game> = HashMap::new();
    let mut romfiles_by_id: HashMap<i64, Romfile> = HashMap::new();
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    for rom in &roms {
        let romfile = find_romfile_by_id(&mut env.connection, rom.romfile_id.unwrap()).await;
        romfiles_by_id.insert(romfile.id, romfile);
    }
    let game = find_game_by_id(&mut env.connection, roms[0].game_id).await;
    games_by_id.insert(roms[0].game_id, game);
    roms_by_game_id.insert(roms[0].game_id, roms);

    let destination_directory = env.tmp_directory.join("destination");
    create_directory(&env.progress_bar, &destination_directory, true)
        .await
        .unwrap();

    // when
    to_archive(
        &mut env.connection,
        &env.progress_bar,
        &destination_directory,
        &system,
        games_by_id,
        roms_by_game_id,
        romfiles_by_id,
        archive::ArchiveType::Zip,
        &ArchiveCompression::Default,
        false,
    )
    .await
    .unwrap();

    // then
    assert!(
        destination_directory
            .join("Test Game (USA, Europe) (DLC).zip")
            .is_file()
    );
}
