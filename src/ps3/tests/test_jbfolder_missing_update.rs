use super::*;

#[tokio::test]
async fn test() {
    // given a JB folder without its update, as most go
    let mut env = TestEnv::new().await;
    let (disc, _update) = setup(&mut env).await;
    import_jbfolder(&mut env, &disc).await;
    let before = romfiles_by_rom(&mut env).await;
    assert_eq!(before.len(), disc.files.len() - 1);

    // when
    env.run(&["convert-roms", "-f", "ORIGINAL", "-a"])
        .await
        .unwrap();

    // then nothing changes: the ISO cannot be rebuilt without it
    assert_eq!(romfiles_by_rom(&mut env).await, before);
    for path in before.values() {
        assert!(path.exists());
    }
    // and a JB folder without its update stays complete, as before
    assert_eq!(game(&mut env).await.completion, 2);
}
