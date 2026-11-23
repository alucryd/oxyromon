use crate::import_roms::UnattendedMode;
use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let dat_path = env.fixture_source("Test System (20200721).dat");
    let (datfile_xml, detector_xml) = parse_dat(&env.progress_bar, &dat_path, false)
        .await
        .unwrap();

    import_dat(
        &mut env.connection,
        &env.progress_bar,
        &datfile_xml,
        &detector_xml,
        None,
        None,
        false,
        false,
    )
    .await
    .unwrap();

    let system = env.system().await;

    for name in [
        "Test Game (USA, Europe) (Multiple Tracks).cue",
        "Test Game (USA, Europe) (Multiple Tracks).chd",
    ] {
        let romfile_path = env.fixture(name).await;
        import_rom(
            &mut env.connection,
            &env.progress_bar,
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
    let dat_path = env.fixture_source("Test System (20240627) (Renamed Tracks Shared).dat");
    let (datfile_xml, detector_xml) = parse_dat(&env.progress_bar, &dat_path, false)
        .await
        .unwrap();

    // when
    import_dat(
        &mut env.connection,
        &env.progress_bar,
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
    let games = find_games(&mut env.connection).await;
    let romfiles = find_romfiles(&mut env.connection).await;

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
        fs::read(env.rom_directory.join(&cue_romfile.path))
            .await
            .unwrap(),
        fs::read(env.fixture_source("Renamed Game (USA, Europe).cue"))
            .await
            .unwrap()
    );
}
