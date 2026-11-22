use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    let env = TestEnv::new().await;
    let base_path = env.tmp_directory.join("base.rom");
    fs::copy(
        env.fixture_source("Test Game (USA, Europe).rom"),
        &base_path,
    )
    .await
    .unwrap();
    let patch_path = env.tmp_directory.join("patch.xdelta");
    fs::copy(
        env.fixture_source("Test Game (USA, Europe).xdelta"),
        &patch_path,
    )
    .await
    .unwrap();

    let base = CommonRomfile::from_path(&base_path).unwrap();
    let patch = CommonRomfile::from_path(&patch_path)
        .unwrap()
        .as_xdelta()
        .unwrap();

    let dest = env.tmp_directory.join("out");
    fs::create_dir(&dest).await.unwrap();
    let result = patch.patch(&env.progress_bar, &base, &dest).await.unwrap();

    let expected = fs::read(env.fixture_source("Test Game (USA, Europe) (patched).rom"))
        .await
        .unwrap();
    assert_eq!(fs::read(&result.path).await.unwrap(), expected);
    assert!(result.path.starts_with(&dest));

    // a non-xdelta extension is rejected
    assert!(
        CommonRomfile::from_path(&base_path)
            .unwrap()
            .as_xdelta()
            .is_err()
    );
}
