use super::super::import_roms::UnattendedMode;
use super::*;
use crate::test_support::TestEnv;
use std::path::PathBuf;
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

    // Redump renames a game's tracks with it, and its cue sheet, which names them
    let dat_path = env.fixture_source("Test System (20240627) (Renamed Tracks).dat");
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
    let roms = find_roms(&mut env.connection).await;
    let romfiles = find_romfiles(&mut env.connection).await;

    assert_eq!(games.len(), 1);
    assert_eq!(games[0].name, "Renamed Game (USA, Europe)");
    assert_eq!(games[0].completion, Completion::Full as i64);
    assert_eq!(roms.len(), 3);
    assert_eq!(romfiles.len(), 2);

    for romfile in &romfiles {
        assert!(!romfile.path.contains("/Trash/"));
    }

    // the cue sheet names the renamed tracks, and nothing else in it changed
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

    // and the old one is gone
    let cue_paths: Vec<PathBuf> = WalkDir::new(env.rom_directory)
        .into_iter()
        .map(|entry| entry.unwrap().into_path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "cue"))
        .collect();
    assert_eq!(cue_paths.len(), 1);
}
