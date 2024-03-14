use super::super::database::*;
use super::*;
use std::path::PathBuf;
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

    let dat_path = test_directory.join("Test System (20200721).dat");
    let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

    import_dat(
        &mut connection,
        &progress_bar,
        &datfile_xml,
        &detector_xml,
        None,
        false,
        false,
    )
    .await
    .unwrap();

    let system = find_systems(&mut connection).await.remove(0);

    let romfile_names = vec![
        "Test Game (Asia).rom",
        "Test Game (Japan).rom",
        "Test Game (USA, Europe).rom",
    ];
    for romfile_name in romfile_names {
        let romfile_path = tmp_directory.join(romfile_name);
        fs::copy(
            test_directory.join(romfile_name),
            &romfile_path.as_os_str().to_str().unwrap(),
        )
        .await
        .unwrap();
        import_rom(
            &mut connection,
            &progress_bar,
            &Some(&system),
            &None,
            &romfile_path,
            &HashAlgorithm::Crc,
            true,
            true,
            false,
        )
        .await
        .unwrap();
    }

    let dat_path = test_directory.join("Test System (20210401).dat");
    let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

    // when
    import_dat(
        &mut connection,
        &progress_bar,
        &datfile_xml,
        &detector_xml,
        None,
        false,
        false,
    )
    .await
    .unwrap();

    // then
    let systems = find_systems(&mut connection).await;
    assert_eq!(systems.len(), 1);

    let system = systems.first().unwrap();
    assert_eq!(system.name, "Test System");

    let games = find_games(&mut connection).await;
    let roms = find_roms(&mut connection).await;
    let romfiles = find_romfiles(&mut connection).await;

    assert_eq!(games.len(), 3);
    assert_eq!(roms.len(), 3);
    assert_eq!(romfiles.len(), 3);

    let game = games.first().unwrap();
    let rom = roms.first().unwrap();
    let romfile = romfiles.get(2).unwrap();

    assert_eq!(game.name, "Test Game (Asia)");
    assert_eq!(rom.name, "Test Game (Asia).rom");
    assert!(rom.romfile_id.is_none());
    assert!(romfile.path.contains("/Trash/"));

    let game = games.get(1).unwrap();
    let rom = roms.get(2).unwrap();
    let romfile = romfiles.get(1).unwrap();

    assert_eq!(game.name, "Test Game (USA, Europe)");
    assert_eq!(rom.name, "Updated Test Game (USA, Europe).rom");
    assert!(rom.romfile_id.is_some());
    assert_eq!(rom.romfile_id.unwrap(), romfile.id);

    let game = games.get(2).unwrap();
    let rom = roms.get(1).unwrap();
    let romfile = romfiles.first().unwrap();

    assert_eq!(game.name, "Updated Test Game (Japan)");
    assert_eq!(rom.name, "Test Game (Japan).rom");
    assert!(rom.romfile_id.is_some());
    assert_eq!(rom.romfile_id.unwrap(), romfile.id);
}
