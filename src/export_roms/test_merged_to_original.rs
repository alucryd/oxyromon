use super::super::rebuild_roms::merged_set::*;
use super::*;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    let tmp_directory =
        set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    import_split_set(&mut connection, &progress_bar, &tmp_directory).await;
    rebuild(&mut connection, &progress_bar, "MERGED").await;
    let destination_directory = tmp_directory.join("destination");
    std::fs::create_dir(&destination_directory).unwrap();

    // when
    let matches = subcommand().get_matches_from([
        "export-roms",
        "-f",
        "ORIGINAL",
        "-s",
        "Test System (MAME Merged)",
        "-d",
        destination_directory.to_str().unwrap(),
    ]);
    main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    // then each game gets its own directory, with the files under their own names
    for (game, rom, fixture) in [
        (
            "Test Parent",
            "Test Program.rom",
            "Test Game (USA, Europe).rom",
        ),
        ("Test Parent", "Test Common.rom", "Test Game (Japan).rom"),
        (
            "Test Clone 1",
            "Test Program.rom",
            "Test Game (USA) (Rev 1).rom",
        ),
        (
            "Test Clone 1",
            "Test Clones.rom",
            "Test Game (USA, Europe) (Beta).rom",
        ),
        (
            "Test Clone 2",
            "Test Clones.rom",
            "Test Game (USA, Europe) (Beta).rom",
        ),
    ] {
        let path = destination_directory.join(game).join(rom);
        assert_eq!(
            std::fs::read(&path).unwrap_or_else(|_| panic!("Missing {:?}", path)),
            std::fs::read(test_directory.join(fixture)).unwrap()
        );
    }
    assert!(
        !destination_directory
            .join("Test Clone 1")
            .join("Test Clone 1")
            .exists()
    );
}
