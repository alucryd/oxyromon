use super::*;
use std::path::Path;
use tempfile::TempDir;
use tokio::fs;

/// A LaserDisc capture goes to a CHD and back without chdman, and the CHD
/// made from what comes back is the same.
#[tokio::test]
async fn test() {
    let test_directory = Path::new("tests").canonicalize().unwrap();
    let progress_bar = ProgressBar::hidden();
    let dir = TempDir::new_in(&test_directory).unwrap();
    let riff_path = dir.path().join("disc.riff");
    fs::copy(
        test_directory.join("Test Game (USA, Europe) (LD).riff"),
        &riff_path,
    )
    .await
    .unwrap();

    let riff = CommonRomfile::from_path(&riff_path)
        .unwrap()
        .as_riff()
        .await
        .unwrap();
    let first = dir.path().join("first");
    fs::create_dir(&first).await.unwrap();
    let chd = riff
        .to_chd(&progress_bar, &first, &[], &None, None)
        .await
        .unwrap();
    let chd = chd.romfile.as_chd().await.unwrap();
    assert!(chd.chd_type == ChdType::Ld);
    assert!(chd.size > 0);

    let extracted = dir.path().join("extracted");
    fs::create_dir(&extracted).await.unwrap();
    let riff = chd.to_riff(&progress_bar, &extracted).await.unwrap();

    let second = dir.path().join("second");
    fs::create_dir(&second).await.unwrap();
    let again = riff
        .to_chd(&progress_bar, &second, &[], &None, None)
        .await
        .unwrap();
    assert_eq!(
        fs::read(&chd.romfile.path).await.unwrap(),
        fs::read(&again.romfile.path).await.unwrap()
    );
}
