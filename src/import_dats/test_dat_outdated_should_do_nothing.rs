use super::super::database::*;
use super::*;
use tempfile::NamedTempFile;

#[async_std::test]
async fn test() {
    // given
    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let dat_path = test_directory.join("Test System (20200721).dat");
    let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

    import_dat(
        &mut connection,
        &progress_bar,
        &datfile_xml,
        &detector_xml,
        false,
        false,
    )
    .await
    .unwrap();

    let dat_path = test_directory.join("Test System (20000000).dat");
    let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

    // when
    import_dat(
        &mut connection,
        &progress_bar,
        &datfile_xml,
        &detector_xml,
        false,
        false,
    )
    .await
    .unwrap();

    // then
    let systems = find_systems(&mut connection).await;
    assert_eq!(systems.len(), 1);

    let system = systems.get(0).unwrap();
    assert_eq!(system.name, "Test System");

    assert_eq!(find_games(&mut connection).await.len(), 6);
    assert_eq!(find_roms(&mut connection).await.len(), 8);
}
