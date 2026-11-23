extern crate wiremock;

use super::*;
use crate::test_support::TestEnv;
use std::io::{Cursor, Write};
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
    let mut env = TestEnv::new().await;
    let system_name = "Test System";

    // an empty ZIP is an error, and nothing is imported
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/datfile/TS"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_with(&[])))
        .mount(&mock_server)
        .await;
    assert!(
        download_redump_dat(
            &mut env.connection,
            &env.progress_bar,
            &mock_server.uri(),
            system_name,
            false,
            None,
        )
        .await
        .is_err()
    );
    assert!(find_systems(&mut env.connection).await.is_empty());

    // so is a ZIP with more than one entry
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/datfile/TS"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_with(&[
            ("a.dat", b"<xml/>" as &[u8]),
            ("b.dat", b"<xml/>" as &[u8]),
        ])))
        .mount(&mock_server)
        .await;
    assert!(
        download_redump_dat(
            &mut env.connection,
            &env.progress_bar,
            &mock_server.uri(),
            system_name,
            false,
            None,
        )
        .await
        .is_err()
    );
    assert!(find_systems(&mut env.connection).await.is_empty());

    // and an HTTP error status
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/datfile/TS"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not a zip"))
        .mount(&mock_server)
        .await;
    assert!(
        download_redump_dat(
            &mut env.connection,
            &env.progress_bar,
            &mock_server.uri(),
            system_name,
            false,
            None,
        )
        .await
        .is_err()
    );
}
