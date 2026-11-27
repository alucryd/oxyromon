use super::*;

#[tokio::test]
async fn test() {
    // given the disc imported as Redump's ISO, its IRD kept
    let mut env = TestEnv::new().await;
    let (disc, _update) = setup(&mut env).await;
    let iso = env.tmp_directory.join(format!("{GAME}.iso"));
    std::fs::write(&iso, &disc.iso).unwrap();
    env.import_roms(&[&iso]).await;
    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    // when
    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();

    // then it is an ORE, decrypted with the key its IRD gives: smaller than
    // one written without it
    let ore = system_directory.join(format!("{GAME}.ore"));
    assert_eq!(romfiles_by_rom(&mut env).await[&format!("{GAME}.iso")], ore);
    let keyless = env.tmp_directory.join("keyless.ore");
    std::fs::write(&iso, &disc.iso).unwrap();
    ore_rs::compress(
        &iso,
        &keyless,
        &ore_rs::Options::default(),
        &[],
        &mut |_| {},
    )
    .unwrap();
    let (size, keyless) = (
        std::fs::metadata(&ore).unwrap().len(),
        std::fs::metadata(&keyless).unwrap().len(),
    );
    assert!(
        size + 4000 < keyless,
        "{size} against {keyless} without its key"
    );
    // which checks against the DAT
    env.run(&["check-roms", "-a"]).await.unwrap();
    assert_eq!(romfiles_by_rom(&mut env).await[&format!("{GAME}.iso")], ore);

    // and comes back byte for byte
    env.run(&["convert-roms", "-f", "ORIGINAL", "-a"])
        .await
        .unwrap();
    let iso = system_directory.join(format!("{GAME}.iso"));
    assert!(std::fs::read(&iso).unwrap() == disc.iso, "the ISO differs");
}
