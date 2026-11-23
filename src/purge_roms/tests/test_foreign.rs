use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let romfile_path = env
        .fixture_into(&env.rom_directory, "Test Game (USA, Europe).rom")
        .await;

    // when
    purge_foreign_romfiles(&mut env.connection, &env.progress_bar, true)
        .await
        .unwrap();

    // then
    assert!(!romfile_path.is_file());
}
