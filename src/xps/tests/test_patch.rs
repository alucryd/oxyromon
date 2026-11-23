use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

#[tokio::test]
async fn test() {
    let env = TestEnv::new().await;
    // base rom and a BPS patch, both at absolute paths
    let base_path = env.tmp_directory.join("base.rom");
    fs::copy(
        env.fixture_source("Test Game (USA, Europe).rom"),
        &base_path,
    )
    .await
    .unwrap();
    let patch_path = env.tmp_directory.join("patch.bps");
    fs::copy(
        env.fixture_source("Test Game (USA, Europe).bps"),
        &patch_path,
    )
    .await
    .unwrap();

    let base = CommonRomfile::from_path(&base_path).unwrap();
    let patch = CommonRomfile::from_path(&patch_path)
        .unwrap()
        .as_xps()
        .unwrap();
    assert_eq!(
        xps_rs::identify(&patch.romfile.path).unwrap(),
        xps_rs::Format::Bps
    );

    let dest = env.tmp_directory.join("out");
    fs::create_dir(&dest).await.unwrap();
    let result = patch.patch(&env.progress_bar, &base, &dest).await.unwrap();

    let expected = fs::read(env.fixture_source("Test Game (USA, Europe) (patched).rom"))
        .await
        .unwrap();
    assert_eq!(fs::read(&result.path).await.unwrap(), expected);
    assert!(result.path.starts_with(&dest));

    // an IPS patch is recognised as one
    let ips_path = env.tmp_directory.join("patch.ips");
    fs::copy(env.fixture_source("Test Game (USA, Europe).ips"), &ips_path)
        .await
        .unwrap();
    let ips = CommonRomfile::from_path(&ips_path)
        .unwrap()
        .as_xps()
        .unwrap();
    assert_eq!(
        xps_rs::identify(&ips.romfile.path).unwrap(),
        xps_rs::Format::Ips
    );

    // a non-patch extension is rejected
    assert!(
        CommonRomfile::from_path(&base_path)
            .unwrap()
            .as_xps()
            .is_err()
    );
}
