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

    // when
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "7Z", "-a", "-c"]);
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();

    // then the title's folder becomes one archive in its place, holding every
    // file, its contents included
    let game_directory = system_directory.join("Test Game (USA) (Update)");
    assert!(!game_directory.exists());
    let archive = system_directory.join("Test Game (USA) (Update).7z");
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    assert_eq!(
        romfiles[0].path,
        archive
            .strip_prefix(&env.rom_directory)
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
    let mut entries: Vec<String> = CommonRomfile::from_path(&archive)
        .unwrap()
        .as_archive(&env.progress_bar, None)
        .await
        .unwrap()
        .into_iter()
        .map(|archive_romfile| archive_romfile.path)
        .collect();
    entries.sort();
    assert_eq!(
        entries,
        [
            "00000010",
            "00000011",
            "00000012",
            "00000012.h3",
            "cetk.48",
            "tmd.48",
        ]
    );
}
