use super::super::database::*;
use super::super::import_dats;
use super::super::import_roms;
use super::*;
use std::path::{Path, PathBuf};
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

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    set_rom_directory(PathBuf::from(rom_directory.path()));
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    let tmp_directory = set_tmp_directory(PathBuf::from(tmp_directory.path()));

    let matches = import_dats::subcommand()
        .get_matches_from(&["import-dats", "tests/Test System (20210402) (Headered).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Headered).rom");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (Headered).rom"),
        &romfile_path,
    )
    .await
    .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let header = find_header_by_system_id(&mut connection, system.id).await;

    import_roms::import_rom(
        &mut connection,
        &progress_bar,
        &Some(&system),
        &header,
        &romfile_path,
        true,
        false,
        false,
        &None,
    )
    .await
    .unwrap();

    let games = find_complete_games_by_system_id(&mut connection, system.id).await;

    // when
    check_system(&mut connection, &progress_bar, &system, games, false)
        .await
        .unwrap();

    // then
    let mut romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);

    let romfile = romfiles.remove(0);
    assert!(!romfile.path.contains("/Trash/"));
    assert!(&rom_directory.path().join(&romfile.path).is_file());
}
