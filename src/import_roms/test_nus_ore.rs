use super::super::convert_roms;
use super::super::import_dats;
use super::*;
use std::path::PathBuf;
use tempfile::TempDir;

#[tokio::test]
async fn test() {
    // given a title's ORE, made by one collection
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();
    let elsewhere = TempDir::new_in(test_directory).unwrap();
    let ore_path = elsewhere.path().join("Somewhere Else.ore");
    {
        let db_file = crate::config::TestDatabase::new();
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
        let matches =
            subcommand().get_matches_from(["import-roms", update_directory.to_str().unwrap()]);
        main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();
        let matches =
            convert_roms::subcommand().get_matches_from(["convert-roms", "-f", "ORE", "-a"]);
        convert_roms::main(&mut connection, &matches, &progress_bar)
            .await
            .unwrap();
        let system = find_systems(&mut connection).await.remove(0);
        let system_directory = get_system_directory(&mut connection, &system)
            .await
            .unwrap();
        std::fs::copy(
            system_directory.join("Test Game (USA) (Update).ore"),
            &ore_path,
        )
        .unwrap();
    }

    // and another collection that has only the DAT
    let db_file = crate::config::TestDatabase::new();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();
    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;
    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20261025) (CDN).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // when
    let matches = subcommand().get_matches_from(["import-roms", ore_path.to_str().unwrap()]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then the ORE is the title's, under the game's name, every ROM in it
    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();
    let ore = system_directory.join("Test Game (USA) (Update).ore");
    assert!(ore.is_file());
    assert!(!ore_path.exists());
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
}
