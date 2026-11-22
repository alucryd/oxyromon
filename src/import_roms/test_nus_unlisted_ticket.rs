use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20261025) (CDN No Ticket).dat").await;

    // a downloader's names, and its ticket, which the DAT lists no more than DLC's
    let nus_directory = env.tmp_directory.join("Test Game (USA) (Update)");
    std::fs::create_dir(&nus_directory).unwrap();
    let fixture = env.fixture_source("Test Game (USA) (Update)");
    for entry in std::fs::read_dir(&fixture).unwrap() {
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
    let system_directory = env.system_directory(&system).await;

    // when, given the folder holding the title's folder
    env.import_roms(&[&env.tmp_directory.clone()]).await;

    // then every file is in the title's folder, under the DAT's name
    let romfiles = env.romfiles().await;
    assert_eq!(romfiles.len(), 5);
    let game_directory = system_directory.join("Test Game (USA) (Update)");
    for name in ["00000010", "00000011", "00000012", "00000012.h3", "tmd.48"] {
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

    // and the ticket is kept apart, by title ID
    let ticket = env
        .tmp_directory
        .join("wiiu_tickets")
        .join("0005000e10101010.tik");
    let expected = std::fs::read(
        env.fixture_source("Test Game (USA) (Update)")
            .join("cetk.48"),
    )
    .unwrap();
    assert_eq!(std::fs::read(&ticket).unwrap(), expected);
    assert!(!nus_directory.join("title.tik").exists());
    assert!(!game_directory.join("title.tik").exists());
    assert!(nus_directory.join("title.cert").is_file());

    // when the title was imported without it, importing its folder again
    // keeps the ticket
    std::fs::remove_file(&ticket).unwrap();
    std::fs::copy(
        game_directory.join("tmd.48"),
        nus_directory.join("title.tmd"),
    )
    .unwrap();
    std::fs::write(nus_directory.join("title.tik"), &expected).unwrap();
    env.import_roms(&[&nus_directory]).await;
    assert_eq!(std::fs::read(&ticket).unwrap(), expected);
    assert!(!nus_directory.join("title.tik").exists());
}
