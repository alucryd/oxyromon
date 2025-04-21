use super::super::config::*;
use super::super::database::*;
use super::super::import_dats;
use super::*;
use std::env;
use std::path::PathBuf;
use tempfile::{NamedTempFile, TempDir};

#[tokio::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    // TODO: Audit that the environment access only happens in single-threaded code.
    unsafe {
        env::set_var(
            "PATH",
            format!(
                "{}:{}",
                test_directory.as_os_str().to_str().unwrap(),
                env::var("PATH").unwrap()
            ),
        )
    };
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand()
        .get_matches_from(&["import-dats", "tests/Test System (20240704) (IRD).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let ird_path = test_directory.join("Test Game (USA).ird");

    let game = find_games(&mut connection).await.remove(0);

    let (ird_file, mut header) = parse_ird(&ird_path).await.unwrap();

    // when
    import_ird(
        &mut connection,
        &progress_bar,
        &game,
        &ird_file,
        &mut header,
    )
    .await
    .unwrap();

    // then
    let roms = find_roms(&mut connection).await;
    assert_eq!(roms.len(), 208);
}
