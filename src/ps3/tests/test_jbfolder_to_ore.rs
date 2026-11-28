use super::*;

#[tokio::test]
async fn test() {
    // given a JB folder and its update
    let mut env = TestEnv::new().await;
    let (disc, update) = setup(&mut env).await;
    import_jbfolder(&mut env, &disc).await;
    env.import_roms(&[&update]).await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    // when it goes straight to ORE
    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();

    // then the game is the ORE of its ISO, and the JB folder is gone
    let romfiles = romfiles_by_rom(&mut env).await;
    let ore = system_directory.join(format!("{GAME}.ore"));
    assert_eq!(romfiles.len(), 1);
    assert_eq!(romfiles[&format!("{GAME}.iso")], ore);
    assert!(!system_directory.join(GAME).exists());
    assert!(!system_directory.join(format!("{GAME}.iso")).exists());
    assert_eq!(game(&mut env).await.completion, 2);

    // and it comes back as the JB folder, through the ISO
    env.run(&["convert-roms", "-f", "ORIGINAL", "-a"])
        .await
        .unwrap();
    env.run(&["convert-roms", "-f", "JB", "-a"]).await.unwrap();
    let romfiles = romfiles_by_rom(&mut env).await;
    assert_eq!(romfiles.len(), disc.files.len());
    assert!(
        std::fs::read(
            system_directory
                .join(GAME)
                .join("PS3_GAME/USRDIR/EBOOT.BIN")
        )
        .unwrap()
            == disc.file("PS3_GAME/USRDIR/EBOOT.BIN")
    );
}
