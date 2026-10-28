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
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "7Z", "-a", "-c"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then the title's folder becomes one archive in its place, holding every
    // file, its contents included
    let game_directory = system_directory.join("Test Game (USA) (Update)");
    assert!(!game_directory.exists());
    let archive = system_directory.join("Test Game (USA) (Update).7z");
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    assert_eq!(
        romfiles[0].path,
        archive
            .strip_prefix(&rom_directory)
            .unwrap()
            .to_str()
            .unwrap()
    );
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 6);
    assert!(
        roms.iter()
            .all(|rom| rom.romfile_id == Some(romfiles[0].id))
    );
    let mut entries: Vec<String> = CommonRomfile::from_path(&archive)
        .unwrap()
        .as_archive(&progress_bar, None)
        .await
        .unwrap()
        .into_iter()
        .map(|archive_romfile| archive_romfile.path)
        .collect();
    entries.sort();
    assert_eq!(
        entries,
        [
            "00000010",
            "00000011",
            "00000012",
            "00000012.h3",
            "cetk.48",
            "tmd.48",
        ]
    );
}
