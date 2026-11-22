use crate::database::find_header_by_system_id;
use crate::import_roms::{UnattendedMode, import_rom};
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env =
        TestEnv::with_dat("Test System (20210402) (Headered).dat").await;
    let romfile_path = env
        .fixture("Test Game (USA, Europe) (Headered).rom")
        .await;

    let system = env.system().await;
    let header = find_header_by_system_id(&mut env.connection, system.id).await;
    import_rom(
        &mut env.connection,
        &env.progress_bar,
        &Some(&system),
        &header,
        &romfile_path,
        true,
        false,
        false,
        UnattendedMode::Skip,
        false,
    )
    .await
    .unwrap();

    // when
    env.run(&["check-roms", "-a"]).await.unwrap();

    // then
    let romfile = env.single_romfile().await;
    assert!(!romfile.path.contains("/Trash/"));
    assert!(env.romfile_path(&romfile).is_file());
}
