use super::super::config::*;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::path::PathBuf;
use tempfile::{NamedTempFile, TempDir};
use tokio::fs;

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

    for dat in [
        "tests/Test System (20200721).dat",
        "tests/Test System (20230105) (Multiple Discs).dat",
    ] {
        let matches = import_dats::subcommand().get_matches_from(["import-dats", dat]);
        import_dats::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();
    }

    let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom");
    fs::copy(
        test_directory.join("Test Game (USA, Europe).rom"),
        &romfile_path,
    )
    .await
    .unwrap();
    let matches = import_roms::subcommand().get_matches_from([
        "import-roms",
        "-s",
        "Test System",
        romfile_path.as_os_str().to_str().unwrap(),
    ]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // when
    let matches =
        super::subcommand().get_matches_from(["purge-systems", "--system", "Test System"]);
    super::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then: only the system named exactly is purged, without a prompt
    let systems = find_systems(&mut connection).await;
    assert_eq!(systems.len(), 1);
    assert_eq!(systems[0].name, "Test System (Multiple Discs)");

    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    assert!(romfiles[0].path.contains("Trash"));
}
