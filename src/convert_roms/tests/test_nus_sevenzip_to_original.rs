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
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "7Z", "-a"]);
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();

    // when
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORIGINAL", "-a", "-c"]);
    main(&mut env.connection, &matches, &env.progress_bar)
        .await
        .unwrap();

    // then the files go back in the title's folder, not beside the archive
    assert!(
        !system_directory
            .join("Test Game (USA) (Update).7z")
            .exists()
    );
    let game_directory = system_directory.join("Test Game (USA) (Update)");
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 6);
    for name in [
        "00000010",
        "00000011",
        "00000012",
        "00000012.h3",
        "cetk.48",
        "tmd.48",
    ] {
        let path = game_directory.join(name);
        assert!(path.is_file(), "{}", path.display());
        let relative = path
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(romfiles.iter().any(|romfile| romfile.path == relative));
    }
}
