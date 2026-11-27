use super::*;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let (disc, update) = setup(&mut env).await;
    import_jbfolder(&mut env, &disc).await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    // when
    env.import_roms(&[&update]).await;

    // then the update is the JB folder's, kept apart for every game built on its firmware
    let romfiles = romfiles_by_rom(&mut env).await;
    let shared = system_directory.join("PS3_UPDATE/4.90/PS3UPDAT.PUP");
    assert_eq!(romfiles[ps3::UPDATE_PATH], shared);
    assert!(std::fs::read(&shared).unwrap() == disc.file(ps3::UPDATE_PATH));
    assert!(!update.exists());
    assert_eq!(romfiles.len(), disc.files.len());
    // and the JB folder's other files are in its folder
    assert_eq!(
        romfiles["PS3_GAME/USRDIR/EBOOT.BIN"],
        system_directory
            .join(GAME)
            .join("PS3_GAME/USRDIR/EBOOT.BIN")
    );

    // when the same update comes again
    std::fs::write(&update, disc.file(ps3::UPDATE_PATH)).unwrap();
    env.import_roms(&[&update]).await;

    // then it is already imported, and left where it is
    assert!(update.exists());
    assert_eq!(romfiles_by_rom(&mut env).await[ps3::UPDATE_PATH], shared);

    // and a game sorted or purged elsewhere would not take it along
    env.run(&["sort-roms", "-a", "-y"]).await.unwrap();
    assert_eq!(romfiles_by_rom(&mut env).await[ps3::UPDATE_PATH], shared);
}
