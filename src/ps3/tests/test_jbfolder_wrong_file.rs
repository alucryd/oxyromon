use super::*;

#[tokio::test]
async fn test() {
    // given a JB folder whose file was damaged since it was imported
    let mut env = TestEnv::new().await;
    let (disc, update) = setup(&mut env).await;
    import_jbfolder(&mut env, &disc).await;
    env.import_roms(&[&update]).await;
    let before = romfiles_by_rom(&mut env).await;
    env.corrupt(&before["PS3_GAME/USRDIR/EBOOT.BIN"]).await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    // when
    env.run(&["convert-roms", "-f", "ORIGINAL", "-a"])
        .await
        .unwrap();

    // then the JB folder is kept, and no ISO, whole or partial, is left
    assert_eq!(romfiles_by_rom(&mut env).await, before);
    assert!(!system_directory.join(format!("{GAME}.iso")).exists());
    assert!(!system_directory.join(format!("{GAME}.iso.part")).exists());
}
