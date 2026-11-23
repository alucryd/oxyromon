use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    let env = TestEnv::new().await;
    let nsz_path = env.tmp_directory.join("Test Game (USA).nsz");
    fs::copy(env.fixture_source("Test Game (USA).nsz"), &nsz_path)
        .await
        .unwrap();
    let dest = env.tmp_directory.join("out");
    fs::create_dir(&dest).await.unwrap();

    let nsz = CommonRomfile::from_path(&nsz_path)
        .unwrap()
        .as_nsz()
        .unwrap();
    let nsp = nsz.to_nsp(&env.progress_bar, &dest).await.unwrap();

    let expected = fs::read(env.fixture_source("Test Game (USA).nsp"))
        .await
        .unwrap();
    assert_eq!(fs::read(&nsp.romfile.path).await.unwrap(), expected);
    assert!(nsp.romfile.path.starts_with(&dest));
}
