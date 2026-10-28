use super::super::import_dats;
use super::*;
use std::path::PathBuf;
use tempfile::{NamedTempFile, TempDir};

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand().get_matches_from([
        "import-dats",
        "tests/Test System (20261025) (CDN Missing H3).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // a downloader's names, an H3 file the DAT leaves out, and a file it lacks
    let nus_directory = tmp_directory.join("Test Game (USA) (Update)");
    std::fs::create_dir(&nus_directory).unwrap();
    for entry in std::fs::read_dir(test_directory.join("Test Game (USA) (Update)")).unwrap() {
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

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    // when, given the folder holding the title's folder
    let matches = subcommand().get_matches_from(["import-roms", tmp_directory.to_str().unwrap()]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then every file is in the title's folder, under the DAT's name
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 5);
    let game_directory = system_directory.join("Test Game (USA) (Update)");
    for name in ["00000010", "00000011", "00000012", "cetk.48", "tmd.48"] {
        let path = game_directory.join(name);
        assert!(path.is_file(), "{}", path.display());
        let relative = path.strip_prefix(&rom_directory).unwrap().to_str().unwrap();
        assert!(romfiles.iter().any(|romfile| romfile.path == relative));
    }
    let games = find_full_games_by_system_id(&mut connection, system.id).await;
    assert_eq!(games[0].completion, 2);

    // and what it did not list is left be, the H3 file included
    assert!(nus_directory.join("title.cert").is_file());
    assert!(nus_directory.join("00000012.h3").is_file());
    assert!(!game_directory.join("00000012.h3").exists());
}
