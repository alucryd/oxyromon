use super::super::archive::{ArchiveCompression, ArchiveType, ToArchive};
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
    // in an archive named unlike the game, its tracks too, so no name picks their ROMs
    let mut archive_romfile = None;
    for (name, entry) in [
        (
            "Test Game (USA, Europe) (Identical Tracks).cue",
            "Tracks.cue",
        ),
        (
            "Test Game (USA, Europe) (Identical Tracks) (Track 01).bin",
            "1.bin",
        ),
        (
            "Test Game (USA, Europe) (Identical Tracks) (Track 02).bin",
            "2.bin",
        ),
        (
            "Test Game (USA, Europe) (Identical Tracks) (Track 03).bin",
            "3.bin",
        ),
    ] {
        fs::rename(source_directory.join(name), source_directory.join(entry))
            .await
            .unwrap();
        archive_romfile = Some(
            CommonRomfile::from_path(&source_directory.join(entry))
                .unwrap()
                .to_archive(
                    &progress_bar,
                    &source_directory,
                    &tmp_directory,
                    "Tracks",
                    &ArchiveType::Zip,
                    &ArchiveCompression::Default,
                    false,
                )
                .await
                .unwrap(),
        );
    }
    let romfile_path = archive_romfile.unwrap().romfile.path;
    fs::remove_dir_all(&source_directory).await.unwrap();

    // when
    let matches = subcommand().get_matches_from([
        "import-roms",
        "-u",
        "skip",
        romfile_path.as_os_str().to_str().unwrap(),
    ]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then every ROM, Track 03's included, has its file
    let roms = find_roms_with_romfile_by_system_id(&mut connection, system.id).await;
    assert_eq!(roms.len(), 4);
    let games = find_games(&mut connection).await;
    assert_eq!(games[0].completion, Completion::Full as i64);
}
