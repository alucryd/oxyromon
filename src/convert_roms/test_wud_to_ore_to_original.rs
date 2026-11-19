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

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20261025) (WUD).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let romfile_path = tmp_directory.join("Test Game (USA).wud");
    fs::copy(test_directory.join("Test Game (USA).wud"), &romfile_path)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    let matches = import_roms::subcommand()
        .get_matches_from(["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // when
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a", "-c"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 1);
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    let rom = roms.first().unwrap();
    assert_eq!(rom.name, "Test Game (USA).iso");
    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA).ore")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert_eq!(rom.romfile_id, Some(romfile.id));

    // when
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORIGINAL", "-a", "-c"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then the disc as the DAT names it, byte for byte
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    let iso = rom_directory.path().join(&romfiles[0].path);
    assert_eq!(iso, system_directory.join("Test Game (USA).iso"));
    assert!(
        std::fs::read(&iso).unwrap()
            == std::fs::read(test_directory.join("Test Game (USA).wud")).unwrap()
    );
}
