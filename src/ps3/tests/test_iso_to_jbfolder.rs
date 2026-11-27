use super::*;

#[tokio::test]
async fn test() {
    // given the disc imported as Redump's ISO
    let mut env = TestEnv::new().await;
    let (disc, _update) = setup(&mut env).await;
    let iso = env.tmp_directory.join(format!("{GAME}.iso"));
    std::fs::write(&iso, &disc.iso).unwrap();
    env.import_roms(&[&iso]).await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    // as any other disc, not in a folder of its own
    assert_eq!(
        romfiles_by_rom(&mut env).await[&format!("{GAME}.iso")],
        system_directory.join(format!("{GAME}.iso"))
    );
    assert_eq!(game(&mut env).await.completion, 2);

    // when
    env.run(&["convert-roms", "-f", "JB", "-a"]).await.unwrap();

    // then every file is the JB folder's, its update kept apart
    let romfiles = romfiles_by_rom(&mut env).await;
    assert_eq!(romfiles.len(), disc.files.len());
    for (path, bytes) in &disc.files {
        let expected = if path == ps3::UPDATE_PATH {
            system_directory.join("PS3_UPDATE/4.90/PS3UPDAT.PUP")
        } else {
            system_directory.join(GAME).join(path)
        };
        assert_eq!(romfiles[path], expected);
        assert!(
            std::fs::read(&expected).unwrap() == *bytes,
            "{path} differs"
        );
    }
    // the ISO is gone
    assert!(!system_directory.join(format!("{GAME}.iso")).exists());
    assert_eq!(game(&mut env).await.completion, 2);

    // and it converts back, byte for byte
    env.run(&["convert-roms", "-f", "ORIGINAL", "-a"])
        .await
        .unwrap();
    let iso = system_directory.join(format!("{GAME}.iso"));
    assert!(std::fs::read(&iso).unwrap() == disc.iso, "the ISO differs");
}
