use super::super::import_dats;
use super::super::prompt::SCRIPTED_SELECTIONS;
use super::*;
use std::path::PathBuf;
use tempfile::{NamedTempFile, TempDir};
use tokio::fs;

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

    // both clones have this file, so importing it asks which one it is
    let matches = import_dats::subcommand().get_matches_from([
        "import-dats",
        "tests/Test System (20200721) (MAME Merged).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();
    let romfile_path = tmp_directory.join("Unknown.rom");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (Beta).rom"),
        &romfile_path,
    )
    .await
    .unwrap();

    // when picking "None"
    SCRIPTED_SELECTIONS.lock().unwrap().push_back(Some(0));
    let matches = subcommand().get_matches_from(["import-roms", romfile_path.to_str().unwrap()]);
    let result = main(&mut connection, &matches, &progress_bar).await;
    let unanswered = SCRIPTED_SELECTIONS.lock().unwrap().drain(..).count();

    // then the file is skipped
    result.unwrap();
    assert_eq!(unanswered, 0, "the ROM prompt was never reached");
    assert!(romfile_path.is_file());
    assert!(find_romfiles(&mut connection).await.is_empty());
}
