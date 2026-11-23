use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe).rom").await;

    env.import_roms(&[&romfile_path]).await;

    env.run(&["sort-roms", "-a", "-y", "-o", "JP"])
        .await
        .unwrap();

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();

    // when
    let romfiles = find_romfiles_in_trash(&mut env.connection).await;
    purge_romfiles(
        &mut env.connection,
        &env.progress_bar,
        true,
        "trashed",
        romfiles,
    )
    .await
    .unwrap();

    // then
    let romfiles = find_romfiles(&mut env.connection).await;
    assert!(romfiles.is_empty());
    assert!(
        &system_directory
            .join("Trash")
            .read_dir()
            .unwrap()
            .next()
            .is_none()
    );
}
