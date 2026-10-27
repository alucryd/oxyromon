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

    // the entry also lists an older version's TMD and a content only it uses
    let matches = import_dats::subcommand().get_matches_from([
        "import-dats",
        "tests/Test System (20261025) (CDN History).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // the newest version, whole
    let nus_directory = tmp_directory.join("Test Game (USA) (Update)");
    std::fs::create_dir(&nus_directory).unwrap();
    for entry in std::fs::read_dir(test_directory.join("Test Game (USA) (Update)")).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, nus_directory.join(path.file_name().unwrap())).unwrap();
    }

    let system = find_systems(&mut connection).await.remove(0);

    // when
    let matches = subcommand().get_matches_from(["import-roms", tmp_directory.to_str().unwrap()]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then the older TMD and the content only it uses are ignored
    let games = find_games_by_system_id(&mut connection, system.id).await;
    assert_eq!(games[0].completion, Completion::Full as i64);
}
