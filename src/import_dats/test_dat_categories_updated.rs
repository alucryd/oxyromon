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

    for dat_name in [
        "Test System (20200721) (Parent-Clone).dat",
        "Test System (20261109) (Parent-Clone) (Categories).dat",
    ] {
        let dat_path = test_directory.join(dat_name);
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
    }

    // then
    let games = find_games(&mut connection).await;
    assert_eq!(games.len(), 6);
    let game = games
        .iter()
        .find(|game| game.name == "Test Game (USA, Europe) (Beta)")
        .unwrap();
    assert_eq!(game.categories.as_deref(), Some("Games|Preproduction"));
    let game = games
        .iter()
        .find(|game| game.name == "42 Test Game (USA, Europe)")
        .unwrap();
    assert_eq!(game.categories, None);
}
