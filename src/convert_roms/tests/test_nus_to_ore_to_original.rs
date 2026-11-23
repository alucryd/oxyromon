use crate::import_dats;
use crate::import_roms;
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
    let game_directory = system_directory.join("Test Game (USA) (Update)");
    let original = |name: &str| {
        std::fs::read(test_directory.join("Test Game (USA) (Update)").join(name)).unwrap()
    };

    // when
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a", "-c"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then the title's folder becomes one ORE in its place, its ticket in it
    assert!(!game_directory.exists());
    let ore = system_directory.join("Test Game (USA) (Update).ore");
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    assert_eq!(
        romfiles[0].path,
        ore.strip_prefix(&rom_directory).unwrap().to_str().unwrap()
    );
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 6);
    assert!(
        roms.iter()
            .all(|rom| rom.romfile_id == Some(romfiles[0].id))
    );

    // when
    let matches = subcommand().get_matches_from(["convert-roms", "-f", "ORIGINAL", "-a", "-c"]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then the folder comes back, file for file, a romfile a ROM
    assert!(!ore.exists());
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 6);
    for rom in &roms {
        let romfile = romfiles
            .iter()
            .find(|romfile| Some(romfile.id) == rom.romfile_id)
            .unwrap();
        let path = rom_directory.path().join(&romfile.path);
        assert_eq!(path, game_directory.join(&rom.name));
        assert!(std::fs::read(&path).unwrap() == original(&rom.name));
    }
}
