use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given a Wii U disc and its update both stored as ORE
    let mut env = TestEnv::with_dat("Test System (20261025) (WUD).dat").await;
    env.import_dats(&["Test System (20261025) (CDN).dat"]).await;

    let update_directory = env.fixture("Test Game (USA) (Update)").await;
    env.run(&["import-roms", update_directory.to_str().unwrap()])
        .await
        .unwrap();
    let wud_path = env.fixture("Test Game (USA).wud").await;
    env.import_roms(&[&wud_path]).await;

    env.run(&["convert-roms", "-f", "ORE", "-a"]).await.unwrap();

    let system = find_systems(&mut env.connection)
        .await
        .into_iter()
        .find(|system| system.name == "Test System (WUD)")
        .unwrap();
    let destination_directory = env.tmp_directory.join("destination");
    create_directory(&env.progress_bar, &destination_directory, true)
        .await
        .unwrap();

    // when
    env.run(&[
        "export-roms",
        "-s",
        &system.name,
        "-f",
        "WUA",
        "-d",
        destination_directory.to_str().unwrap(),
    ])
    .await
    .unwrap();

    // then a standalone plain WUA holding both the disc game and its ORE update
    let wua_path = destination_directory.join("Test Game (USA).wua");
    let wua = fs::read(&wua_path).await.unwrap();
    let holds = |name: &[u8]| wua.windows(name.len()).any(|w| w == name);
    assert!(holds(b"0005000010101010_v32"));
    assert!(holds(b"0005000e10101010_v48"));
    assert!(!holds(b".nus"));
    assert!(!holds(b".wud"));
    assert_eq!(wux_rs::disc_info(&wua_path).unwrap(), None);
    assert!(!destination_directory.join(wux_rs::STORE_DIR).exists());
}
