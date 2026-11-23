extern crate wiremock;

use super::*;
use crate::test_support::TestEnv;
use tokio::io::AsyncReadExt;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let zip_path = env.fixture_source("Test System (20200721).zip");
    let mut zip_data = vec![];
    open_file(&zip_path)
        .await
        .unwrap()
        .read_to_end(&mut zip_data)
        .await
        .unwrap();

    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/datfile/[A-Z0-9-]+$"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(zip_data))
        .mount(&mock_server)
        .await;

    // when
    download_redump_dats(
        &mut env.connection,
        &env.progress_bar,
        &mock_server.uri(),
        true,
        env.tmp_directory.to_str(),
    )
    .await
    .unwrap();

    // then
    let systems = find_systems(&mut env.connection).await;
    assert_eq!(systems.len(), 1);

    let system = systems.first().unwrap();
    assert_eq!(system.name, "Test System");

    assert_eq!(find_games(&mut env.connection).await.len(), 6);
    assert_eq!(find_roms(&mut env.connection).await.len(), 8);

    assert!(
        env.tmp_directory
            .join("Test System (20200721).dat")
            .exists()
    );

    // the same DAT again changes nothing, unless forced
    let system_name = *REDUMP_SYSTEMS_CODES.keys().next().unwrap();
    for (force, expected) in [(false, DatOutcome::UpToDate), (true, DatOutcome::Imported)] {
        let outcome = download_redump_dat(
            &mut env.connection,
            &env.progress_bar,
            &mock_server.uri(),
            system_name,
            force,
            None,
        )
        .await
        .unwrap();
        assert_eq!(outcome, expected);
    }
}
