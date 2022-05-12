extern crate wiremock;

use super::super::config::*;
use super::super::database::*;
use super::super::import_dats;
use super::*;
use async_std::fs;
use async_std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};
use wiremock::matchers::{method, path};
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

    let matches = import_dats::subcommand()
        .get_matches_from(&["import-dats", "tests/Test System (20200721).dat"]);
    import_dats::main(&mut connection, &matches, &progress_bar)
        .await
        .unwrap();

    let profile_xml_path = test_directory.join("profile.xml");

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/profile.xml"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(fs::read_to_string(&profile_xml_path).await.unwrap()),
        )
        .mount(&mock_server)
        .await;

    // when
    update_nointro_dats(&mut connection, &progress_bar, &mock_server.uri(), true)
        .await
        .unwrap();

    // then
    //do nothing
}
