use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let system_directory = env.tmp_directory.join("Test System");
    std::fs::create_dir(&system_directory).unwrap();
    // a legacy lossless WUA, self-contained
    let keys = wux_rs::load_keys(&env.fixture_source("Test Game (USA).keys.txt")).unwrap();
    wux_rs::to_wua(
        &[&env.fixture_source("Test Game (USA).wux")],
        &system_directory.join("Test Game (USA).wua"),
        &keys,
        None,
        &mut |_| {},
    )
    .unwrap();
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

    // then the WUA is listed as the disc it holds, and its store is not a game
    let dat = std::fs::read_to_string(dat_directory.join("Test System (42).dat")).unwrap();
    assert!(dat.contains(
        r#"<rom name="Test Game (USA).iso" size="1867776" crc="d2066103" md5="286d9857c9a92545a989267065423ed6" sha1="1adab7c09cc400e9f07a31abed691a0965ff66df"/>"#
    ), "{dat}");
    assert_eq!(dat.matches("<game ").count(), 1, "{dat}");
}
