use super::super::config::*;
use super::super::import_roms::UnattendedMode;
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

    let dat_path = test_directory.join("Test System (20200721).dat");
    let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

    import_dat(
        &mut connection,
        &progress_bar,
        &datfile_xml,
        &detector_xml,
        None,
        None,
        false,
        false,
    )
    .await
    .unwrap();

    let system = find_systems(&mut connection).await.remove(0);

    for name in [
        "Test Game (USA, Europe) (Multiple Tracks).cue",
        "Test Game (USA, Europe) (Multiple Tracks).chd",
    ] {
        let romfile_path = tmp_directory.join(name);
        fs::copy(test_directory.join(name), &romfile_path)
            .await
            .unwrap();
        import_rom(
            &mut connection,
            &progress_bar,
            &Some(&system),
            &None,
            &romfile_path,
            true,
            false,
            true,
            UnattendedMode::Skip,
            false,
        )
        .await
        .unwrap();
    }

    // the renamed game's Track 02 is also another game's, which sorts first
    let dat_path = test_directory.join("Test System (20240627) (Renamed Tracks Shared).dat");
    let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

    // when
    import_dat(
        &mut connection,
        &progress_bar,
        &datfile_xml,
        &detector_xml,
        None,
        None,
        false,
        false,
    )
    .await
    .unwrap();

    // then
    let games = find_games(&mut connection).await;
    let romfiles = find_romfiles(&mut connection).await;

    assert_eq!(games.len(), 2);
    let game = games
        .iter()
        .find(|game| game.name == "Renamed Game (USA, Europe)")
        .unwrap();
    assert_eq!(game.completion, Completion::Full as i64);
    let other_game = games
        .iter()
        .find(|game| game.name == "Another Game (Japan)")
        .unwrap();
    assert_eq!(other_game.completion, Completion::None as i64);
    assert_eq!(romfiles.len(), 2);

    for romfile in &romfiles {
        assert!(!romfile.path.contains("/Trash/"));
    }

    // the cue sheet names the renamed game's Track 02, not the other game's
    let cue_romfile = romfiles
        .iter()
        .find(|romfile| romfile.path.ends_with(".cue"))
        .unwrap();
    assert!(cue_romfile.path.ends_with("Renamed Game (USA, Europe).cue"));
    assert_eq!(
        fs::read(rom_directory.path().join(&cue_romfile.path))
            .await
            .unwrap(),
        fs::read(test_directory.join("Renamed Game (USA, Europe).cue"))
            .await
            .unwrap()
    );
}
