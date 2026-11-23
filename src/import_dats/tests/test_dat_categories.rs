use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let dat_path = env.fixture_source("Test System (20261109) (Parent-Clone) (Categories).dat");
    let (datfile_xml, detector_xml) = parse_dat(&env.progress_bar, &dat_path, false)
        .await
        .unwrap();

    // when
    import_dat(
        &mut env.connection,
        &env.progress_bar,
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
    let categories: Vec<(String, Option<String>)> = find_games(&mut env.connection)
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
