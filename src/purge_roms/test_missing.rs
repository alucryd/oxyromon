use super::*;
use crate::test_support::TestEnv;
use itertools::Itertools;
use std::fs::DirEntry;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe).rom").await;

    env.import_roms(&[&romfile_path]).await;

    let romfiles = find_romfiles(&mut env.connection).await;
    romfiles[0]
        .as_common(&mut env.connection)
        .await
        .unwrap()
        .delete(&env.progress_bar, false)
        .await
        .unwrap();

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();

    // when
    purge_missing_romfiles(&mut env.connection, &env.progress_bar)
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
}
