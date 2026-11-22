use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let nsz_path = env.fixture("Test Game (USA).nsz").await;
    let nsz = CommonRomfile::from_path(&nsz_path)
        .unwrap()
        .as_nsz()
        .unwrap();

    let rom = Rom {
        id: 1,
        name: "Test Game (USA).nsp".to_string(),
        bios: false,
        disk: false,
        size: 40946,
        crc: None,
        md5: None,
        sha1: Some("8dfa6a7045f8bd56f8c5d33c20a48b07659ecb76".to_string()),
        rom_status: None,
        game_id: 1,
        romfile_id: None,
        parent_id: None,
        original: true,
    };

    // when
    let result = nsz
        .check(&mut env.connection, &env.progress_bar, &None, &[&rom])
        .await;

    // then
    assert!(result.is_ok());
}
