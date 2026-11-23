use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    // Renamed Game and Another Game share their Track 02
    env.import_dats(&["Test System (20240627) (Renamed Tracks Shared).dat"])
        .await;

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();

    // named neither as a ROM nor as a game, but closest to Renamed Game's Track 02
    let romfile_path = env.tmp_directory.join("renamed game (track 02).bin");
    fs::copy(
        env.fixture_source("Test Game (USA, Europe) (CUE BIN) (Track 02).bin"),
        &romfile_path,
    )
    .await
    .unwrap();

    // when
    env.run(&["import-roms", "-u", "first", romfile_path.to_str().unwrap()])
        .await
        .unwrap();

    // then it is Renamed Game's
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(
        roms.iter()
            .map(|rom| rom.name.as_str())
            .collect::<Vec<&str>>(),
        ["Renamed Game (USA, Europe) (Track 02).bin"]
    );
    let romfile = find_romfile_by_id(&mut env.connection, roms[0].romfile_id.unwrap()).await;
    assert_eq!(
        romfile.path,
        system_directory
            .join("Renamed Game (USA, Europe) (Track 02).bin")
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
}
