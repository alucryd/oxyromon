use super::super::config::set_string;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use crate::test_support::TestEnv;
use itertools::Itertools;
use std::fs::DirEntry;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    set_string(&mut env.connection, "GAME_SUBFOLDERS", "always", None).await;

    env.import_dats(&["Test System (20200721).dat"]).await;

    let romfile_path = env.fixture("Test Game (USA, Europe).rom").await;

    env.import_roms(&[&romfile_path]).await;

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();

    let game_directory = system_directory.join("Test Game (USA, Europe)");
    assert!(game_directory.is_dir());

    delete_game_by_name_and_system_id(&mut env.connection, "Test Game (USA, Europe)", system.id)
        .await;

    // when
    let romfiles = find_orphan_romfiles(&mut env.connection).await;
    purge_romfiles(
        &mut env.connection,
        &env.progress_bar,
        true,
        "orphan",
        romfiles,
    )
    .await
    .unwrap();

    // then
    let romfiles = find_romfiles(&mut env.connection).await;
    assert!(romfiles.is_empty());
    let entries: Vec<DirEntry> = system_directory
        .read_dir()
        .unwrap()
        .map_ok(|entry| entry)
        .try_collect()
        .unwrap();
    assert_eq!(entries.len(), 0);
    assert!(!game_directory.exists());
}
