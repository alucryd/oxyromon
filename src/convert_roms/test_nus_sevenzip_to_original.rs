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
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "7Z", "-a"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // when
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORIGINAL", "-a", "-c"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then the files go back in the title's folder, not beside the archive
    assert!(
        !system_directory
            .join("Test Game (USA) (Update).7z")
            .exists()
    );
    let game_directory = system_directory.join("Test Game (USA) (Update)");
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 6);
    for name in [
        "00000010",
        "00000011",
        "00000012",
        "00000012.h3",
        "cetk.48",
        "tmd.48",
    ] {
        let path = game_directory.join(name);
        assert!(path.is_file(), "{}", path.display());
        let relative = path.strip_prefix(&rom_directory).unwrap().to_str().unwrap();
        assert!(romfiles.iter().any(|romfile| romfile.path == relative));
    }
}
