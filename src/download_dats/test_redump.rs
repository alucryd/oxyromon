extern crate wiremock;

use super::super::config::*;
use super::super::database::*;
use super::super::util::*;
use super::*;
use async_std::io::prelude::*;
use async_std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[async_std::test]
async fn test() {
    // given
    let _guard = MUTEX.lock().await;

    let test_directory = Path::new("tests");
    let progress_bar = ProgressBar::hidden();

    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let rom_directory = TempDir::new_in(&test_directory).unwrap();
    set_rom_directory(PathBuf::from(rom_directory.path()));
    let tmp_directory = TempDir::new_in(&test_directory).unwrap();
    set_tmp_directory(PathBuf::from(tmp_directory.path()));

    let zip_path = test_directory.join("Test System (20200721).zip");
    let mut zip_data = Vec::new();
    open_file(&zip_path)
        .await
        .unwrap()
        .read_to_end(&mut zip_data)
        .await
        .unwrap();

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/datfile/[a-z0-9-]+/$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_data))
        .mount(&mock_server)
        .await;

    // when
    download_redump_dats(&mut connection, &progress_bar, &mock_server.uri(), true)
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
