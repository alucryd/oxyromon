use crate::import_roms::UnattendedMode;
use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let dat_path = env.fixture_source("Test System (20200721) (Identical Tracks).dat");
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

    // the cue sheet alone: its tracks are paired by the ROMs of the DATs, and Track 03 is Track
    // 02's bytes
    let name = "Test Game (USA, Europe) (Identical Tracks).cue";
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

    let dat_path = env.fixture_source("Test System (20240627) (Renamed Identical Tracks).dat");
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
    assert_eq!(
        games[0].name,
        "Renamed Game (USA, Europe) (Identical Tracks)"
    );

    // the cue sheet names Track 02 and Track 03 in that order, each once
    let cue_romfile = romfiles
        .iter()
        .find(|romfile| romfile.path.ends_with(".cue"))
        .unwrap();
    assert!(!cue_romfile.path.contains("/Trash/"));
    assert!(
        cue_romfile
            .path
            .ends_with("Renamed Game (USA, Europe) (Identical Tracks).cue")
    );
    assert_eq!(
        fs::read(env.rom_directory.join(&cue_romfile.path))
            .await
            .unwrap(),
        fs::read(env.fixture_source("Renamed Game (USA, Europe) (Identical Tracks).cue"))
            .await
            .unwrap()
    );
}
