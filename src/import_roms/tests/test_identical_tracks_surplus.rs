use super::*;
use crate::chd::ToChd;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721) (Identical Tracks).dat").await;

    let system = env.system().await;

    // Tracks 03 and 04 are Track 02's bytes too: one copy more than the DAT lists
    let source_directory = env.tmp_directory.join("source");
    fs::create_dir(&source_directory).await.unwrap();
    let track_name =
        |track: u8| format!("Test Game (USA, Europe) (Identical Tracks) (Track 0{track}).bin");
    let mut cue = format!(
        "FILE \"{}\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 00 00:00:00\n    INDEX 01 00:03:06\n",
        track_name(1)
    );
    fs::copy(
        env.fixture_source("Test Game (USA, Europe) (CUE BIN) (Track 01).bin"),
        source_directory.join(track_name(1)),
    )
    .await
    .unwrap();
    for track in 2..=4 {
        cue.push_str(&format!(
            "FILE \"{}\" BINARY\n  TRACK 0{track} AUDIO\n    INDEX 01 00:00:00\n",
            track_name(track)
        ));
        fs::copy(
            env.fixture_source("Test Game (USA, Europe) (CUE BIN) (Track 02).bin"),
            source_directory.join(track_name(track)),
        )
        .await
        .unwrap();
    }
    let cue_path = source_directory.join("Test Game (USA, Europe) (Identical Tracks).cue");
    fs::write(&cue_path, cue).await.unwrap();
    let chd_romfile = CommonRomfile::from_path(&cue_path)
        .unwrap()
        .as_cue_bin(
            (1..=4)
                .map(|track| {
                    CommonRomfile::from_path(&source_directory.join(track_name(track))).unwrap()
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
        "--trash",
        "-u",
        "skip",
        chd_romfile.romfile.path.to_str().unwrap(),
    ])
    .await
    .unwrap();

    // then it is not the DAT's disc, as with a track the DAT doesn't list: no ROM points to it,
    // and it is trashed
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 0);
    let romfiles = find_romfiles(&mut env.connection).await;
    assert_eq!(romfiles.len(), 1);
    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        env.rom_directory
            .join("Trash")
            .join("Invalid")
            .join("Test Game (USA, Europe) (Identical Tracks).chd")
            .strip_prefix(&env.rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(env.rom_directory.join(&romfile.path).is_file());
}
