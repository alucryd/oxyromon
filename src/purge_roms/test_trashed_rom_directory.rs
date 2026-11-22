use super::super::import_roms::{UnattendedMode, import_other};
use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    // an invalid ROM matches no system, so `import-roms -t` trashes it in the ROM
    // directory's own Trash, as a path relative to it: Trash/Invalid/<name>
    let romfile_path = env.fixture("Test Game (USA, Europe) (Headered).rom").await;

    let system = env.system().await;
    import_other(
        &mut env.connection,
        &env.progress_bar,
        &Some(&system),
        &None,
        &HashSet::new(),
        CommonRomfile::from_path(&romfile_path).unwrap(),
        true,
        false,
        UnattendedMode::Skip,
    )
    .await
    .unwrap();

    let trash_directory = env.rom_directory.join("Trash");
    assert_eq!(find_romfiles(&mut env.connection).await.len(), 1);
    assert!(
        trash_directory
            .join("Invalid")
            .read_dir()
            .unwrap()
            .next()
            .is_some()
    );

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
    assert!(trash_directory.is_dir());
    assert!(trash_directory.read_dir().unwrap().next().is_none());
}
