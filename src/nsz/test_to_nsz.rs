use super::*;
use std::path::Path;
use tempfile::TempDir;
use tokio::fs;

#[tokio::test]
async fn test() {
    let test_directory = Path::new("tests").canonicalize().unwrap();
    let progress_bar = ProgressBar::hidden();
    let dir = TempDir::new_in(&test_directory).unwrap();

    let nsp_path = dir.path().join("Test Game (USA).nsp");
    fs::copy(test_directory.join("Test Game (USA).nsp"), &nsp_path)
        .await
        .unwrap();
    let dest = dir.path().join("out");
    fs::create_dir(&dest).await.unwrap();

    let nsp = CommonRomfile::from_path(&nsp_path)
        .unwrap()
        .as_nsp()
        .unwrap();
    let nsz = nsp.to_nsz(&progress_bar, &dest).await.unwrap();
    assert_eq!(nsz.romfile.path.extension().unwrap(), NSZ_EXTENSION);

    // The compressed bytes are the compressor's business, the round-trip isn't
    let nsp = nsz.to_nsp(&progress_bar, &dest).await.unwrap();
    let expected = fs::read(test_directory.join("Test Game (USA).nsp"))
        .await
        .unwrap();
    assert_eq!(fs::read(&nsp.romfile.path).await.unwrap(), expected);
}
