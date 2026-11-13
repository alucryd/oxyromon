use super::super::config::*;
use super::*;
use std::path::PathBuf;
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

    let dat_path = test_directory.join("Test System (20261109) (Parent-Clone) (Categories).dat");
    let (datfile_xml, detector_xml) = parse_dat(&progress_bar, &dat_path, false).await.unwrap();

    // when
    import_dat(
        &mut connection,
        &progress_bar,
        &datfile_xml,
        &detector_xml,
        None,
        None,
        false,
        false,
    )
    .await
    .unwrap();

    // then
    let categories: Vec<(String, Option<String>)> = find_games(&mut connection)
        .await
        .into_iter()
        .map(|game| (game.name, game.categories))
        .collect();
    assert_eq!(
        categories,
        vec![
            (String::from("42 Test Game (USA, Europe)"), None),
            (
                String::from("Another Test Game (USA, Europe)"),
                Some(String::from("Applications"))
            ),
            (
                String::from("Test Game (Asia)"),
                Some(String::from("Demos"))
            ),
            (
                String::from("Test Game (Japan)"),
                Some(String::from("Games"))
            ),
            (
                String::from("Test Game (USA, Europe)"),
                Some(String::from("Games"))
            ),
            (
                String::from("Test Game (USA, Europe) (Beta)"),
                Some(String::from("Games|Preproduction"))
            ),
        ]
    );
}
