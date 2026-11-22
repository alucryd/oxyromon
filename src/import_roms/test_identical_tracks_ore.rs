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

    // its Track 03 is Track 02's bytes
    let source_directory = env.tmp_directory.join("source");
    fs::create_dir(&source_directory).await.unwrap();
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
        fs::copy(env.fixture_source(fixture), source_directory.join(name))
            .await
            .unwrap();
    }
    let romfile_path = env
        .tmp_directory
        .join("Test Game (USA, Europe) (Identical Tracks).ore");
    ore_rs::compress(
        &source_directory.join("Test Game (USA, Europe) (Identical Tracks).cue"),
        &romfile_path,
        &ore_rs::Options::default(),
        &[],
        &mut |_| {},
    )
    .unwrap();
    fs::remove_dir_all(&source_directory).await.unwrap();

    // when
    env.run(&["import-roms", "-u", "skip", romfile_path.to_str().unwrap()])
        .await
        .unwrap();

    // then every ROM, Track 03 and the cue sheet included, points to the ORE
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 4);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe) (Identical Tracks).ore")
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(roms.iter().all(|rom| rom.romfile_id == Some(romfile.id)));
    let games = find_games(&mut env.connection).await;
    assert_eq!(games[0].completion, Completion::Full as i64);
}
