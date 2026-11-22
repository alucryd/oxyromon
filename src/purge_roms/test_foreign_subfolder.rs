use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    // given
    let mut env = TestEnv::new().await;
    let subfolder = env.rom_directory.join("Test System").join("T");
    fs::create_dir_all(&subfolder).await.unwrap();
    let romfile_path = env
        .fixture_into(&subfolder, "Test Game (USA, Europe).rom")
        .await;

    // when
    purge_foreign_romfiles(&mut env.connection, &env.progress_bar, true)
        .await
        .unwrap();

    // then
    assert!(!romfile_path.is_file());
    assert!(!env.rom_directory.join("Test System").exists());
    assert!(env.rom_directory.is_dir());
}
