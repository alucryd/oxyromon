use super::super::check_roms;
use super::super::import_dats;
use super::*;
use std::path::PathBuf;
use tempfile::TempDir;
use tokio::fs;

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

    // an ORE of a cue sheet unlike the DAT's: the tracks are Redump's
    let dump_directory = TempDir::new_in(test_directory).unwrap();
    for track in [
        "Test Game (USA, Europe) (CUE BIN) (Track 01).bin",
        "Test Game (USA, Europe) (CUE BIN) (Track 02).bin",
    ] {
        fs::copy(
            test_directory.join(track),
            dump_directory.path().join(track),
        )
        .await
        .unwrap();
    }
    let dat_cue = test_directory.join("Test Game (USA, Europe) (Multiple Tracks).cue");
    let dump_cue = dump_directory
        .path()
        .join("Test Game (USA, Europe) (CUE BIN).cue");
    let text = fs::read_to_string(&dat_cue).await.unwrap();
    fs::write(&dump_cue, format!("REM dumped by hand\r\n{text}"))
        .await
        .unwrap();
    let romfile_path = tmp_directory.join("Test Game (USA, Europe) (CUE BIN).ore");
    ore_rs::compress(
        &dump_cue,
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

    let matches =
        subcommand().get_matches_from(["import-roms", romfile_path.as_os_str().to_str().unwrap()]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // its tracks point to the ORE, its cue's ROM to nothing yet
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 2);
    assert!(roms.iter().all(|rom| rom.name.ends_with(".bin")));
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    let ore_romfile = romfiles.first().unwrap();

    // when the DAT's cue sheet is imported
    let cue_path = tmp_directory.join("Test Game (USA, Europe) (Multiple Tracks).cue");
    fs::copy(&dat_cue, &cue_path).await.unwrap();
    let matches =
        subcommand().get_matches_from(["import-roms", cue_path.as_os_str().to_str().unwrap()]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then it is in the ORE, which every ROM points to
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 3);
    assert!(
        roms.iter()
            .all(|rom| rom.romfile_id == Some(ore_romfile.id))
    );
    assert_eq!(find_romfiles(&mut connection).await.len(), 1);
    assert!(!cue_path.exists());
    assert!(
        !system_directory
            .join("Test Game (USA, Europe) (CUE BIN).cue")
            .exists()
    );

    // and the ORE gives back the DAT's cue sheet
    let matches = check_roms::subcommand().get_matches_from(["check-roms", "-a"]);
    check_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    assert!(!romfiles[0].path.contains("/Trash/"));
}
