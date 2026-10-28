use super::super::config::*;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::path::{Path, PathBuf};
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

    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20261025) (CDN).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();
    let update_directory = tmp_directory.join("Test Game (USA) (Update)");
    std::fs::create_dir(&update_directory).unwrap();
    for entry in std::fs::read_dir(test_directory.join("Test Game (USA) (Update)")).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, update_directory.join(path.file_name().unwrap())).unwrap();
    }
    let matches = import_roms::subcommand()
        .get_matches_from(["import-roms", update_directory.to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    // when
    let matches = super::subcommand().get_matches_from(["check-roms", "-a"]);
    super::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then every file is checked and passes, contents named without an
    // extension included
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 6);
    assert!(romfiles.iter().all(|romfile| {
        romfile
            .path
            .starts_with("Test System/Test Game (USA) (Update)/")
    }));
    assert!(!system_directory.join("Trash").exists());
}
