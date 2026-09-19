use super::*;
use std::path::Path;
use tempfile::TempDir;
use tokio::fs;

#[tokio::test]
async fn test() {
    // flips is not installed in CI; skip rather than fail.
    if get_version().await.is_err() {
        return;
    }

    let test_directory = Path::new("tests").canonicalize().unwrap();
    let progress_bar = ProgressBar::hidden();
    let dir = TempDir::new_in(&test_directory).unwrap();

    // base rom and a BPS patch, both at absolute paths
    let base_path = dir.path().join("base.rom");
    fs::copy(
        test_directory.join("Test Game (USA, Europe).rom"),
        &base_path,
    )
    .await
    .unwrap();
    let patch_path = dir.path().join("patch.bps");
    fs::copy(
        test_directory.join("Test Game (USA, Europe).bps"),
        &patch_path,
    )
    .await
    .unwrap();

    let base = CommonRomfile::from_path(&base_path).unwrap();
    let patch = CommonRomfile::from_path(&patch_path)
        .unwrap()
        .as_xps()
        .unwrap();
    assert!(patch.xps_type == XpsType::Bps);

    let dest = dir.path().join("out");
    fs::create_dir(&dest).await.unwrap();
    let result = patch.patch(&progress_bar, &base, &dest).await.unwrap();

    let expected = fs::read(test_directory.join("Test Game (USA, Europe) (patched).rom"))
        .await
        .unwrap();
    assert_eq!(fs::read(&result.path).await.unwrap(), expected);
    assert!(result.path.starts_with(&dest));

    // an IPS patch is recognised as the Ips variant
    let ips_path = dir.path().join("patch.ips");
    fs::copy(
        test_directory.join("Test Game (USA, Europe).ips"),
        &ips_path,
    )
    .await
    .unwrap();
    let ips = CommonRomfile::from_path(&ips_path)
        .unwrap()
        .as_xps()
        .unwrap();
    assert!(ips.xps_type == XpsType::Ips);

    // a non-patch extension is rejected
    assert!(
        CommonRomfile::from_path(&base_path)
            .unwrap()
            .as_xps()
            .is_err()
    );
}
