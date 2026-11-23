use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::with_dat("Test System (20200721).dat").await;

    let romfile_path = env.fixture("Test Game (USA, Europe).rom").await;

    // when -s names no system, the ROM matching another one
    let result = env
        .run(&[
            "import-roms",
            "-s",
            "Unknown System",
            romfile_path.to_str().unwrap(),
        ])
        .await;
    // then nothing is looked up in the other systems
    assert_eq!(result.unwrap_err().to_string(), "No matching system found");
    assert!(romfile_path.is_file());
    assert!(find_romfiles(&mut env.connection).await.is_empty());
}
