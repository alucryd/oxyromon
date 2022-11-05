use super::super::config::{set_rom_directory, set_tmp_directory, MUTEX};
use super::super::database::*;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use async_std::fs;
use async_std::path::PathBuf;
use tempfile::{NamedTempFile, TempDir};

#[async_std::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    set_rom_directory(PathBuf::from(rom_directory.path()));
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

    let matches = import_dats::subcommand()
        .get_matches_from(&["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom");
    fs::copy(
        test_directory.join("Test Game (USA, Europe).rom"),
        &romfile_path,
    )
    .await
    .unwrap();

    let system = find_systems(&mut connection).await.remove(0);

    let matches = import_roms::subcommand()
        .get_matches_from(&["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile = find_romfiles(&mut connection).await.remove(0);
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&romfile.path)
        .await
        .unwrap();
    file.set_len(512).await.unwrap();

    // when
    check_system(
        &mut connection,
        &progress_bar,
        &system,
        false,
        &HashAlgorithm::Crc,
    )
    .await
    .unwrap();

    // then
    let mut romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);

    let romfile = romfiles.remove(0);
    assert!(romfile.path.contains("/Trash/"));
    assert!(Path::new(&romfile.path).is_file().await);
}
