use super::super::import_dats;
use super::*;
use std::path::PathBuf;
use tempfile::TempDir;

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = crate::config::TestDatabase::new();
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

    let romfile_path = tmp_directory.join("Test Game (USA, Europe) (Multiple Tracks).ore");
    ore_rs::compress(
        &test_directory.join("Test Game (USA, Europe) (Multiple Tracks).cue"),
        &romfile_path,
        &ore_rs::Options::default(),
        &[],
        &mut |_| {},
    )
    .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    // when
    let matches =
        subcommand().get_matches_from(["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then every ROM, the cue sheet's included, points to the ORE
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 3);
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);

    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe) (CUE BIN).ore")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(rom_directory.path().join(&romfile.path).is_file());
    assert!(roms.iter().all(|rom| rom.romfile_id == Some(romfile.id)));
    assert!(
        !system_directory
            .join("Test Game (USA, Europe) (CUE BIN).cue")
            .exists()
    );
}
