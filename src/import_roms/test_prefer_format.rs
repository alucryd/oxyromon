use super::super::import_dats;
use super::*;
use crate::{convert_roms, import_roms};
use sqlx::SqlitePool;
use std::path::PathBuf;
use tempfile::{NamedTempFile, TempDir};
use tokio::fs;

/// Shared given: temp DB + directories + an imported DAT. The `NamedTempFile`
/// and `TempDir`s must be kept alive by the caller for the whole test.
async fn setup(dat: &str) -> (NamedTempFile, TempDir, TempDir, SqlitePool, i64, PathBuf) {
    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(test_directory).unwrap();
    set_rom_directory(&mut connection, PathBuf::from(rom_directory.path())).await;
    let tmp_directory = TempDir::new_in(test_directory).unwrap();
    set_tmp_directory(&mut connection, PathBuf::from(tmp_directory.path())).await;

    let matches = import_dats::subcommand().get_matches_from(["import-dats", dat]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);
    let system_directory = get_system_directory(&mut connection, &system)
        .await
        .unwrap();

    (
        db_file,
        rom_directory,
        tmp_directory,
        pool,
        system.id,
        system_directory,
    )
}

async fn import_one(pool: &SqlitePool, extra: &[&str]) {
    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();
    let mut connection = pool.acquire().await.unwrap();

    let tmp_directory = get_tmp_directory(&mut connection).await;
    let romfile_path = tmp_directory.join("Test Game (USA, Europe).rom");
    fs::copy(
        test_directory.join("Test Game (USA, Europe).rom"),
        &romfile_path,
    )
    .await
    .unwrap();

    let mut args = vec!["import-roms"];
    args.extend(extra);
    args.push(romfile_path.as_os_str().to_str().unwrap());
    let matches = import_roms::subcommand().get_matches_from(&args);
    import_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_prefer_format_converts_on_import() {
    let _guard = MUTEX.lock().await;
    let (_db, _roms, _tmp, pool, system_id, system_directory) =
        setup("tests/Test System (20200721).dat").await;
    let mut connection = pool.acquire().await.unwrap();
    set_string(&mut connection, "PREFER_FORMAT", "7Z", Some(system_id)).await;
    drop(connection);

    import_one(&pool, &[]).await;

    let mut connection = pool.acquire().await.unwrap();
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    let path = PathBuf::from(&romfiles[0].path);
    assert_eq!(path.extension().unwrap(), "7z");
    let expected = system_directory
        .strip_prefix(_roms.path())
        .unwrap()
        .join("Test Game (USA, Europe).7z");
    assert_eq!(path, expected);
    assert!(_roms.path().join(&path).is_file());
}

#[tokio::test]
async fn test_no_prefer_format_leaves_roms_untouched() {
    let _guard = MUTEX.lock().await;
    let (_db, _roms, _tmp, pool, _system_id, _system_directory) =
        setup("tests/Test System (20200721).dat").await;

    import_one(&pool, &[]).await;

    let mut connection = pool.acquire().await.unwrap();
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    let path = PathBuf::from(&romfiles[0].path);
    assert_eq!(path.extension().unwrap(), "rom");
}

#[tokio::test]
async fn test_as_is_skips_auto_conversion() {
    let _guard = MUTEX.lock().await;
    let (_db, _roms, _tmp, pool, system_id, _system_directory) =
        setup("tests/Test System (20200721).dat").await;
    let mut connection = pool.acquire().await.unwrap();
    set_string(&mut connection, "PREFER_FORMAT", "7Z", Some(system_id)).await;
    drop(connection);

    import_one(&pool, &["--as-is"]).await;

    let mut connection = pool.acquire().await.unwrap();
    let romfiles = find_romfiles(&mut connection).await;
    assert_eq!(romfiles.len(), 1);
    let path = PathBuf::from(&romfiles[0].path);
    assert_eq!(path.extension().unwrap(), "rom");
}

#[tokio::test]
async fn test_convert_save_persists_prefer_format() {
    let _guard = MUTEX.lock().await;
    let (_db, _roms, _tmp, pool, system_id, _system_directory) =
        setup("tests/Test System (20200721).dat").await;

    import_one(&pool, &[]).await;

    let progress_bar = ProgressBar::hidden();
    let mut connection = pool.acquire().await.unwrap();
    let matches = convert_roms::subcommand().get_matches_from([
        "convert-roms",
        "--all",
        "--format",
        "7Z",
        "--save",
    ]);
    convert_roms::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let format = get_string(&mut connection, "PREFER_FORMAT", Some(system_id)).await;
    assert_eq!(format.as_deref(), Some("7Z"));

    let romfiles = find_romfiles(&mut connection).await;
    let path = PathBuf::from(&romfiles[0].path);
    assert_eq!(path.extension().unwrap(), "7z");
}

#[tokio::test]
async fn test_arcade_rejects_unsupported_prefer_format() {
    let _guard = MUTEX.lock().await;
    let (_db, _roms, _tmp, pool, system_id, _system_directory) =
        setup("tests/Test System (20200721) (MAME).dat").await;
    let progress_bar = ProgressBar::hidden();
    let mut connection = pool.acquire().await.unwrap();

    let system = find_system_by_id(&mut connection, system_id).await;
    assert!(system.arcade);

    set_setting(
        &mut connection,
        &progress_bar,
        "PREFER_FORMAT",
        "CHD",
        Some(system_id),
    )
    .await
    .unwrap();
    let format = get_string(&mut connection, "PREFER_FORMAT", Some(system_id)).await;
    assert_eq!(format, None);

    set_setting(
        &mut connection,
        &progress_bar,
        "PREFER_FORMAT",
        "ZIP",
        Some(system_id),
    )
    .await
    .unwrap();
    let format = get_string(&mut connection, "PREFER_FORMAT", Some(system_id)).await;
    assert_eq!(format.as_deref(), Some("ZIP"));
}
