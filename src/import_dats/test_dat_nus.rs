use super::super::config::*;
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

    // when
    let matches = super::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20261025) (CDN).dat"]);
    super::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then a NUS title's contents, named by their bare ID, are original, as
    // are its TMD, ticket and H3 file
    let roms = find_roms(&mut connection).await;
    assert_eq!(roms.len(), 6);
    for rom in &roms {
        assert!(rom.original, "{}", rom.name);
    }
}
