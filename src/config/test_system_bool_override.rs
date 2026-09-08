use super::super::import_dats;
use super::*;
use indicatif::ProgressBar;
use tempfile::NamedTempFile;

#[tokio::test]
async fn test() {
    // given
    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();
    let progress_bar = ProgressBar::hidden();

    let matches = import_dats::subcommand()
        .get_matches_from(["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let system = find_systems(&mut connection).await.remove(0);

    // when
    set_bool(&mut connection, "PREFER_PARENTS", true, None).await;
    set_bool(&mut connection, "PREFER_PARENTS", false, Some(system.id)).await;

    // then
    assert!(!get_bool(&mut connection, "PREFER_PARENTS", Some(system.id)).await);
    assert!(get_bool(&mut connection, "PREFER_PARENTS", None).await);
}
