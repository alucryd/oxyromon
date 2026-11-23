use super::*;
use crate::import_roms::UnattendedMode;
use crate::test_support::TestEnv;

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

    let dat_path = env.fixture_source("Test System (20240627) (Renamed Tracks Mismatch).dat");
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
    assert_eq!(games[0].completion, Completion::Partial as i64);
    assert_eq!(romfiles.len(), 2);

    // the tracks match, but the renamed cue sheet isn't the one the DAT lists
    for romfile in &romfiles {
        assert_eq!(
            romfile.path.contains("/Trash/"),
            romfile.path.ends_with(".cue")
        );
    }
}
