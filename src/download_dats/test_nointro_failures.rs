extern crate wiremock;

use super::*;
use tempfile::NamedTempFile;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A profile that cannot be fetched is an error, not a panic or a silent no-op.
#[tokio::test]
async fn test() {
    // given
    let progress_bar = ProgressBar::hidden();
    let db_file = NamedTempFile::new().unwrap();
    let pool = establish_connection(db_file.path().to_str().unwrap()).await;
    let mut connection = pool.acquire().await.unwrap();

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/profile.xml"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&mock_server)
        .await;

    // when an HTTP error status, then an unreachable host
    for base_url in [mock_server.uri(), String::from("http://127.0.0.1:9")] {
        let result = update_nointro_dats(&mut connection, &progress_bar, &base_url, true).await;

        // then
        assert!(result.is_err(), "{base_url}");
    }
}
