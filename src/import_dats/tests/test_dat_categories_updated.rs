use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    for dat_name in [
        "Test System (20200721) (Parent-Clone).dat",
        "Test System (20261109) (Parent-Clone) (Categories).dat",
    ] {
        let dat_path = env.fixture_source(dat_name);
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
    }

    // then
    let games = find_games(&mut env.connection).await;
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
