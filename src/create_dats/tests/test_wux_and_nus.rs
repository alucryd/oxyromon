use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let system_directory = env.tmp_directory.join("Test System");
    std::fs::create_dir(&system_directory).unwrap();
    std::fs::copy(
        env.fixture_source("Test Game (USA).wux"),
        system_directory.join("Test Game (USA).wux"),
    )
    .unwrap();
    let _update_directory = env
        .fixture_into(&system_directory, "Test Game (USA) (Update)")
        .await;
    let dat_directory = env.tmp_directory.join("DAT");

    // when
    create_dat(
        &mut env.connection,
        &env.progress_bar,
        &system_directory,
        Some(&dat_directory),
        None,
        None,
        Some(&String::from("42")),
        None,
        None,
    )
    .await
    .unwrap();

    // then the WUX is listed as the WUD it holds, as in the WUD DAT
    let dat = std::fs::read_to_string(dat_directory.join("Test System (42).dat")).unwrap();
    assert!(dat.contains(
        r#"<rom name="Test Game (USA).iso" size="1867776" crc="d2066103" md5="286d9857c9a92545a989267065423ed6" sha1="1adab7c09cc400e9f07a31abed691a0965ff66df"/>"#
    ), "{dat}");
    // and the update's folder is one game, its files its ROMs, as in the CDN DAT
    assert_eq!(dat.matches("<game ").count(), 2);
    assert!(dat.contains(r#"<game name="Test Game (USA) (Update)">"#));
    let cdn =
        std::fs::read_to_string(env.fixture_source("Test System (20261025) (CDN).dat")).unwrap();
    for line in cdn.lines().filter(|line| line.contains("<rom ")) {
        assert!(dat.contains(line.trim()), "{line}");
    }
}
