use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    let env = TestEnv::new().await;
    let nsp_path = env.tmp_directory.join("Test Game (USA).nsp");
    fs::copy(env.fixture_source("Test Game (USA).nsp"), &nsp_path)
        .await
        .unwrap();
    let dest = env.tmp_directory.join("out");
    fs::create_dir(&dest).await.unwrap();

    let nsp = CommonRomfile::from_path(&nsp_path)
        .unwrap()
        .as_nsp()
        .unwrap();
    let nsz = nsp.to_nsz(&env.progress_bar, &dest).await.unwrap();
    assert_eq!(nsz.romfile.path.extension().unwrap(), NSZ_EXTENSION);

    // The compressed bytes are the compressor's business, the round-trip isn't
    let nsp = nsz.to_nsp(&env.progress_bar, &dest).await.unwrap();
    let expected = fs::read(env.fixture_source("Test Game (USA).nsp"))
        .await
        .unwrap();
    assert_eq!(fs::read(&nsp.romfile.path).await.unwrap(), expected);
}
