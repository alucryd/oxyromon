use super::*;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let (disc, update) = setup(&mut env).await;
    import_jbfolder(&mut env, &disc).await;
    env.import_roms(&[&update]).await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    // when
    env.run(&["convert-roms", "-f", "ORIGINAL", "-a"])
        .await
        .unwrap();

    // then the ISO is Redump's, and the only file the game has
    let romfiles = romfiles_by_rom(&mut env).await;
    let iso = system_directory.join(format!("{GAME}.iso"));
    assert_eq!(romfiles.len(), 1);
    assert_eq!(romfiles[&format!("{GAME}.iso")], iso);
    assert!(std::fs::read(&iso).unwrap() == disc.iso, "the ISO differs");
    // the JB folder, and the update only it used, are gone
    assert!(!system_directory.join(GAME).exists());
    assert!(
        !system_directory
            .join("PS3_UPDATE/4.90/PS3UPDAT.PUP")
            .exists()
    );
    assert_eq!(env.romfiles().await.len(), 1);
    // and the game is whole
    assert_eq!(game(&mut env).await.completion, 2);
}
