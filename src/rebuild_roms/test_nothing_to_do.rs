use super::super::import_dats;
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
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand().get_matches_from([
        "import-dats",
        "tests/Test System (20200721) (MAME Rebuild).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // when rebuilding to the merging strategy the system already uses
    let matches = subcommand().get_matches_from(["rebuild-roms", "--all", "-m", "SPLIT"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then nothing changed
    let system = find_arcade_systems(&mut connection).await.remove(0);
    assert_eq!(system.merging, Merging::Split as i64);
    let romfiles = find_romfiles(&mut connection).await;
    assert!(romfiles.is_empty());
}
