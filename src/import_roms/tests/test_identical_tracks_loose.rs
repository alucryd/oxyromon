use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721) (Identical Tracks).dat").await;

    let system = env.system().await;
    let system_directory = get_system_directory(&mut env.connection, &system)
        .await
        .unwrap();

    // its Track 03 is Track 02's bytes, both named as the DAT names them
    let input_directory = env.tmp_directory.join("input");
    fs::create_dir(&input_directory).await.unwrap();
    for (fixture, name) in [
        (
            "Test Game (USA, Europe) (Identical Tracks).cue",
            "Test Game (USA, Europe) (Identical Tracks).cue",
        ),
        (
            "Test Game (USA, Europe) (CUE BIN) (Track 01).bin",
            "Test Game (USA, Europe) (Identical Tracks) (Track 01).bin",
        ),
        (
            "Test Game (USA, Europe) (CUE BIN) (Track 02).bin",
            "Test Game (USA, Europe) (Identical Tracks) (Track 02).bin",
        ),
        (
            "Test Game (USA, Europe) (CUE BIN) (Track 02).bin",
            "Test Game (USA, Europe) (Identical Tracks) (Track 03).bin",
        ),
    ] {
        fs::copy(env.fixture_source(fixture), input_directory.join(name))
            .await
            .unwrap();
    }

    // when
    env.run(&[
        "import-roms",
        "-u",
        "skip",
        input_directory.to_str().unwrap(),
    ])
    .await
    .unwrap();

    // then every ROM has a file of its own, named as it is
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 4);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 4);
    for rom in &roms {
        let romfile = find_romfile_by_id(&mut env.connection, rom.romfile_id.unwrap()).await;
        assert_eq!(
            romfile.path,
            system_directory
                .join(&rom.name)
                .strip_prefix(&env.rom_directory)
                .unwrap()
                .as_os_str()
                .to_str()
                .unwrap(),
        );
        assert!(env.rom_directory.join(&romfile.path).is_file());
    }
    let games = find_games(&mut env.connection).await;
    assert_eq!(games[0].completion, Completion::Full as i64);
}
