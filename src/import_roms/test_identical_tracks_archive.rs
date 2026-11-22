use super::super::archive::{ArchiveCompression, ArchiveType, ToArchive};
use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721) (Identical Tracks).dat").await;

    let system = env.system().await;

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
                    &env.progress_bar,
                    &source_directory,
                    &env.tmp_directory,
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
    env.run(&["import-roms", "-u", "skip", romfile_path.to_str().unwrap()])
        .await
        .unwrap();

    // then every ROM, Track 03's included, has its file
    let roms = find_roms_with_romfile_by_system_id(&mut env.connection, system.id).await;
    assert_eq!(roms.len(), 4);
    let games = find_games(&mut env.connection).await;
    assert_eq!(games[0].completion, Completion::Full as i64);
}
