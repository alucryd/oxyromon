use super::*;

#[tokio::test]
async fn test() {
    // given the disc as Redump's ISO, its kept IRD since damaged
    let mut env = TestEnv::new().await;
    let (disc, _update) = setup(&mut env).await;
    let iso = env.tmp_directory.join(format!("{GAME}.iso"));
    std::fs::write(&iso, &disc.iso).unwrap();
    env.import_roms(&[&iso]).await;
    let game = game(&mut env).await;
    create_or_replace_ird(&mut env.connection, game.id, b"not an IRD").await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    // when
    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();

    // then it is still converted, stored as it is without its key, whole
    let ore = system_directory.join(format!("{GAME}.ore"));
    assert_eq!(romfiles_by_rom(&mut env).await[&format!("{GAME}.iso")], ore);
    env.run(&["convert-roms", "-f", "ORIGINAL", "-a"])
        .await
        .unwrap();
    let iso = system_directory.join(format!("{GAME}.iso"));
    assert!(std::fs::read(&iso).unwrap() == disc.iso, "the ISO differs");
}
