use super::*;
use std::path::Path;
use tempfile::TempDir;
use tokio::fs;

#[tokio::test]
async fn test() {
    let test_directory = Path::new("tests").canonicalize().unwrap();
    let progress_bar = ProgressBar::hidden();
    let dir = TempDir::new_in(&test_directory).unwrap();

    let nsz_path = dir.path().join("Test Game (USA).nsz");
    fs::copy(test_directory.join("Test Game (USA).nsz"), &nsz_path)
        .await
        .unwrap();
    let dest = dir.path().join("out");
    fs::create_dir(&dest).await.unwrap();

    let nsz = CommonRomfile::from_path(&nsz_path)
        .unwrap()
        .as_nsz()
        .unwrap();
    let nsp = nsz.to_nsp(&progress_bar, &dest).await.unwrap();

    let expected = fs::read(test_directory.join("Test Game (USA).nsp"))
        .await
        .unwrap();
    assert_eq!(fs::read(&nsp.romfile.path).await.unwrap(), expected);
    assert!(nsp.romfile.path.starts_with(&dest));
}
