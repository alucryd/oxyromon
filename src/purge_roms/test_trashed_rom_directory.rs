use super::super::config::{MUTEX, set_rom_directory, set_tmp_directory};
use super::super::import_dats;
use super::super::import_roms::{UnattendedMode, import_other};
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

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // an invalid ROM matches no system, so `import-roms -t` trashes it in the ROM
    // directory's own Trash, as a path relative to it: Trash/Invalid/<name>
    let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Headered).rom");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (Headered).rom"),
        &romfile_path,
    )
    .await
    .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    import_other(
        &mut connection,
        &progress_bar,
        &Some(&system),
        &None,
        &HashSet::new(),
        CommonRomfile::from_path(&romfile_path).unwrap(),
        true,
        false,
        UnattendedMode::Skip,
    )
    .await
    .unwrap();

    let trash_directory = rom_directory.path().join("Trash");
    assert_eq!(find_romfiles(&mut connection).await.len(), 1);
    assert!(
        trash_directory
            .join("Invalid")
            .read_dir()
            .unwrap()
            .next()
            .is_some()
    );

    // when
    let romfiles = find_romfiles_in_trash(&mut connection).await;
    purge_romfiles(&mut connection, &progress_bar, true, "trashed", romfiles)
        .await
        .unwrap();

    // then
    let romfiles = find_romfiles(&mut connection).await;
    assert!(romfiles.is_empty());
    assert!(trash_directory.is_dir());
    assert!(trash_directory.read_dir().unwrap().next().is_none());
}
