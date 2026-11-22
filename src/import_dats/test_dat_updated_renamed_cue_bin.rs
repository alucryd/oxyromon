use super::super::import_roms::UnattendedMode;
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
        "Test Game (USA, Europe) (CUE BIN) (Track 01).bin",
        "Test Game (USA, Europe) (CUE BIN) (Track 02).bin",
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
    let romfiles = find_romfiles(&mut env.connection).await;

    assert_eq!(games.len(), 1);
    assert_eq!(games[0].completion, Completion::Full as i64);
    assert_eq!(romfiles.len(), 3);

    let mut names: Vec<&str> = romfiles
        .iter()
        .map(|romfile| {
            Path::new(&romfile.path)
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
        })
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "Renamed Game (USA, Europe) (Track 01).bin",
            "Renamed Game (USA, Europe) (Track 02).bin",
            "Renamed Game (USA, Europe).cue",
        ]
    );

    // the cue sheet names the tracks as they are now named
    let cue_romfile = romfiles
        .iter()
        .find(|romfile| romfile.path.ends_with(".cue"))
        .unwrap();
    assert_eq!(
        fs::read(env.rom_directory.join(&cue_romfile.path))
            .await
            .unwrap(),
        fs::read(env.fixture_source("Renamed Game (USA, Europe).cue"))
            .await
            .unwrap()
    );
}
