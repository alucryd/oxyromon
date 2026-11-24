use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261025) (WUD).dat").await;

    let romfile_path = env.fixture("Test Game (USA).wud").await;

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;

    env.run(&["import-roms", romfile_path.as_os_str().to_str().unwrap()])
        .await
        .unwrap();

    // when
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a", "-c"]);
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 1);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA).iso");
    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        env.stored_path(&system_directory.join("Test Game (USA).ore")),
    );
    assert_eq!(rom.romfile_id, Some(romfile.id));

    // when
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORIGINAL", "-a", "-c"]);
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();

    // then the disc as the DAT names it, byte for byte
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    let iso = env.rom_directory.join(&romfiles[0].path);
    assert_eq!(iso, system_directory.join("Test Game (USA).iso"));
    assert!(
        std::fs::read(&iso).unwrap()
            == std::fs::read(env.fixture_source("Test Game (USA).wud")).unwrap()
    );
}
