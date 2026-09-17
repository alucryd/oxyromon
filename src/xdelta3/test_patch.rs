use super::*;
use std::path::Path;
use tempfile::TempDir;
use tokio::fs;

#[tokio::test]
async fn test() {
    // xdelta3 is not installed in CI; skip rather than fail.
    if get_version().await.is_err() {
        return;
    }

    let test_directory = Path::new("tests").canonicalize().unwrap();
    let progress_bar = ProgressBar::hidden();
    let dir = TempDir::new_in(&test_directory).unwrap();

    let base_path = dir.path().join("base.rom");
    fs::copy(test_directory.join("Test Game (USA, Europe).rom"), &base_path)
        .await
        .unwrap();
    let patch_path = dir.path().join("patch.xdelta");
    fs::copy(
        test_directory.join("Test Game (USA, Europe).xdelta"),
        &patch_path,
    )
    .await
    .unwrap();

    let base = CommonRomfile::from_path(&base_path).unwrap();
    let patch = CommonRomfile::from_path(&patch_path).unwrap().as_xdelta().unwrap();

    let dest = dir.path().join("out");
    fs::create_dir(&dest).await.unwrap();
    let result = patch.patch(&progress_bar, &base, &dest).await.unwrap();

    let expected = fs::read(test_directory.join("Test Game (USA, Europe) (patched).rom"))
        .await
        .unwrap();
    assert_eq!(fs::read(&result.path).await.unwrap(), expected);
    assert!(result.path.starts_with(&dest));

    // a non-xdelta extension is rejected
    assert!(CommonRomfile::from_path(&base_path).unwrap().as_xdelta().is_err());
}
