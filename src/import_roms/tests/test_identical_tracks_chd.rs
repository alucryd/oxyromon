use super::*;
use crate::chd::ToChd;
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
    let cue_path = source_directory.join("Test Game (USA, Europe) (Identical Tracks).cue");
    let chd_romfile = CommonRomfile::from_path(&cue_path)
        .unwrap()
        .as_cue_bin(
            (1..=3)
                .map(|track| {
                    CommonRomfile::from_path(&source_directory.join(format!(
                        "Test Game (USA, Europe) (Identical Tracks) (Track 0{track}).bin"
                    )))
                    .unwrap()
                })
                .collect(),
        )
        .unwrap()
        .to_chd(&env.progress_bar, &env.tmp_directory, &[], &None, None)
        .await
        .unwrap();
    fs::remove_dir_all(&source_directory).await.unwrap();

    // when
    env.run(&[
        "import-roms",
        "-u",
        "skip",
        chd_romfile.romfile.path.to_str().unwrap(),
    ])
    .await
    .unwrap();

    // then every track, Track 03's included, points to the CHD
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(
        roms.iter()
            .map(|rom| rom.name.as_str())
            .collect::<Vec<&str>>(),
        [
            "Test Game (USA, Europe) (Identical Tracks) (Track 01).bin",
            "Test Game (USA, Europe) (Identical Tracks) (Track 02).bin",
            "Test Game (USA, Europe) (Identical Tracks) (Track 03).bin",
        ]
    );
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe) (Identical Tracks).chd")
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(roms.iter().all(|rom| rom.romfile_id == Some(romfile.id)));
}
