use super::super::config::{MUTEX, set_rom_directory, set_tmp_directory};
use super::super::import_dats;
use super::*;
use std::io::Write;
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
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    // a Redump DAT
    let matches = import_dats::subcommand().get_matches_from([
        "import-dats",
        "tests/Test System (20261111) (Clone Lists).dat",
    ]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);

    // and a No-Intro clone list for a system of the same name
    let mut clone_list_file = NamedTempFile::new_in(tmp_directory.path()).unwrap();
    write!(
        clone_list_file,
        r#"{{"description": {{"name": "Test System (Clone Lists) (No-Intro)", "lastUpdated": "2026-11-11 00:00:00"}}, "variants": [{{"group": "Test Game", "titles": [{{"searchTerm": "Test Game"}}, {{"searchTerm": "Testspiel"}}]}}]}}"#
    )
    .unwrap();

    // when
    let imported = import_clone_list(
        &mut connection,
        &progress_bar,
        clone_list_file.path(),
        false,
    )
    .await
    .unwrap();

    // then
    assert_eq!(imported, None);
    assert!(
        find_clone_list_version_by_system_id(&mut connection, system.id)
            .await
            .is_none()
    );
}
