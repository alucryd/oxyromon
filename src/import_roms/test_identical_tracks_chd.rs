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
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    // its Track 03 is Track 02's bytes
    let source_directory = tmp_directory.join("source");
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
        fs::copy(test_directory.join(fixture), source_directory.join(name))
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
        .to_chd(&progress_bar, &tmp_directory, &[], &None, None)
        .await
        .unwrap();
    fs::remove_dir_all(&source_directory).await.unwrap();

    // when
    let matches = subcommand().get_matches_from([
        "import-roms",
        "-u",
        "skip",
        chd_romfile.romfile.path.as_os_str().to_str().unwrap(),
    ]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then every track, Track 03's included, points to the CHD
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
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
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    let romfile = romfiles.first().unwrap();
    assert_eq!(
        romfile.path,
        system_directory
            .join("Test Game (USA, Europe) (Identical Tracks).chd")
            .strip_prefix(&rom_directory)
            .unwrap()
            .as_os_str()
            .to_str()
            .unwrap(),
    );
    assert!(roms.iter().all(|rom| rom.romfile_id == Some(romfile.id)));
}
