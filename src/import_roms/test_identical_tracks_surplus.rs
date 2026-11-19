use super::super::chd::ToChd;
use super::super::import_dats;
use super::*;
use std::path::PathBuf;
use tempfile::TempDir;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = crate::config::TestDatabase::new();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand().get_matches_from([
        "import-dats",
        "tests/Test System (20200721) (Identical Tracks).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);

    // Tracks 03 and 04 are Track 02's bytes too: one copy more than the DAT lists
    let source_directory = tmp_directory.join("source");
    fs::create_dir(&source_directory).await.unwrap();
    let track_name =
        |track: u8| format!("Test Game (USA, Europe) (Identical Tracks) (Track 0{track}).bin");
    let mut cue = format!(
        "FILE \"{}\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 00 00:00:00\n    INDEX 01 00:03:06\n",
        track_name(1)
    );
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (CUE BIN) (Track 01).bin"),
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
            test_directory.join("Test Game (USA, Europe) (CUE BIN) (Track 02).bin"),
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
        .to_chd(&progress_bar, &tmp_directory, &[], &None, None)
        .await
        .unwrap();
    fs::remove_dir_all(&source_directory).await.unwrap();

    // when
    let matches = subcommand().get_matches_from([
        "import-roms",
        "--trash",
        "-u",
        "skip",
        chd_romfile.romfile.path.as_os_str().to_str().unwrap(),
    ]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then it is not the DAT's disc, as with a track the DAT doesn't list: no ROM points to it,
    // and it is trashed
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 0);
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        rom_directory
            .path()
            .join("Trash")
            .join("Invalid")
            .join("Test Game (USA, Europe) (Identical Tracks).chd")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(rom_directory.path().join(&romfile.path).is_file());
}
