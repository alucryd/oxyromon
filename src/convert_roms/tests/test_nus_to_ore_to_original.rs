use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261025) (CDN).dat").await;

    let update_directory = env.fixture("Test Game (USA) (Update)").await;
    env.run(&["import-roms", update_directory.to_str().unwrap()])
        .await
        .unwrap();

    let system = env.system().await;
    let system_directory = env.system_directory(&system).await;
    let game_directory = system_directory.join("Test Game (USA) (Update)");
    let source_directory = env.fixture_source("Test Game (USA) (Update)");
    let original = |name: &str| std::fs::read(source_directory.join(name)).unwrap();

    // when
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a", "-c"]);
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();

    // then the title's folder becomes one ORE in its place, its ticket in it
    assert!(!game_directory.exists());
    let ore = system_directory.join("Test Game (USA) (Update).ore");
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    assert_eq!(
        romfiles[0].path,
        ore.strip_prefix(&env.rom_directory)
            .unwrap()
            .to_str()
            .unwrap()
    );
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 6);
    assert!(
        roms.iter()
            .all(|rom| rom.romfile_id == Some(romfiles[0].id))
    );

    // when
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORIGINAL", "-a", "-c"]);
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();

    // then the folder comes back, file for file, a romfile a ROM
    assert!(!ore.exists());
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 6);
    for rom in &roms {
        let romfile = romfiles
            .iter()
            .find(|romfile| Some(romfile.id) == rom.romfile_id)
            .unwrap();
        let path = env.rom_directory.join(&romfile.path);
        assert_eq!(path, game_directory.join(&rom.name));
        assert!(std::fs::read(&path).unwrap() == original(&rom.name));
    }
}
