use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261025) (CDN Missing H3).dat").await;

    // a downloader's names, an H3 file the DAT leaves out, and a file it lacks
    let nus_directory = env.tmp_directory.join("Test Game (USA) (Update)");
    std::fs::create_dir(&nus_directory).unwrap();
    for entry in std::fs::read_dir(env.fixture_source("Test Game (USA) (Update)")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        let name = match name {
            "tmd.48" => "title.tmd".to_string(),
            "cetk.48" => "title.tik".to_string(),
            h3 if h3.ends_with(".h3") => h3.to_string(),
            content => format!("{content}.app"),
        };
        std::fs::copy(&path, nus_directory.join(name)).unwrap();
    }
    std::fs::write(nus_directory.join("title.cert"), b"not in the DAT").unwrap();

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();

    // when, given the folder holding the title's folder
    let import_dir = env.tmp_directory.clone();
    env.run(&["import-roms", import_dir.to_str().unwrap()])
        .await
        .unwrap();

    // then every file is in the title's folder, under the DAT's name
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 5);
    let game_directory = system_directory.join("Test Game (USA) (Update)");
    for name in ["00000010", "00000011", "00000012", "cetk.48", "tmd.48"] {
        let path = game_directory.join(name);
        assert!(path.is_file(), "{}", path.display());
        let relative = path
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(romfiles.iter().any(|romfile| romfile.path == relative));
    }
    let games = find_full_games_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(games[0].completion, 2);

    // and what it did not list is left be, the H3 file included
    assert!(nus_directory.join("title.cert").is_file());
    assert!(nus_directory.join("00000012.h3").is_file());
    assert!(!game_directory.join("00000012.h3").exists());
}
