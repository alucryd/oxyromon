extern crate wiremock;

use super::super::config::*;
use super::*;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use zip::write::{SimpleFileOptions, ZipWriter};

fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    for (name, data) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

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

    let system_name = "Test System";

    // an empty ZIP is reported and skipped, not imported
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/datfile/ts/"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_with(&[])))
        .mount(&mock_server)
        .await;
    download_redump_dat(
        &mut connection,
        &progress_bar,
        &mock_server.uri(),
        system_name,
        false,
        None,
    )
    .await
    .unwrap();
    assert!(find_systems(&mut connection).await.is_empty());

    // a ZIP with more than one entry is reported and skipped
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/datfile/ts/"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_with(&[
            ("a.dat", b"<xml/>" as &[u8]),
            ("b.dat", b"<xml/>" as &[u8]),
        ])))
        .mount(&mock_server)
        .await;
    download_redump_dat(
        &mut connection,
        &progress_bar,
        &mock_server.uri(),
        system_name,
        false,
        None,
    )
    .await
    .unwrap();
    assert!(find_systems(&mut connection).await.is_empty());

    // a body that is not a ZIP surfaces as an error
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/datfile/ts/"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not a zip"))
        .mount(&mock_server)
        .await;
    assert!(
        download_redump_dat(
            &mut connection,
            &progress_bar,
            &mock_server.uri(),
            system_name,
            false,
            None,
        )
        .await
        .is_err()
    );
}
