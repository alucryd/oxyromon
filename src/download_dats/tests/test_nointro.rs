extern crate wiremock;

use super::*;
use crate::test_support::TestEnv;
use tokio::fs;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let profile_xml_path = env.fixture_source("profile.xml");

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
    update_nointro_dats(
        &mut env.connection,
        &env.progress_bar,
        &mock_server.uri(),
        true,
    )
    .await
    .unwrap();

    // then
    //do nothing
}
