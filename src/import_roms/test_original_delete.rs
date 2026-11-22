use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe) (Headered).rom").await;

    let system = env.system().await;

    // when
    import_other(
        &mut env.connection,
        &env.progress_bar,
        &Some(&system),
        &None,
        &HashSet::new(),
        CommonRomfile::from_path(&romfile_path).unwrap(),
        false,
        true,
        UnattendedMode::Skip,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 0);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 0);

    assert!(
        !env.rom_directory
            .join("Trash")
            .join("Test Game (USA, Europe) (Headered).rom")
            .is_file()
    );
}
