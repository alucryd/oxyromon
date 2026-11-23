use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

/// A LaserDisc capture goes to a CHD and back without chdman, and the CHD
/// made from what comes back is the same.
#[tokio::test]
async fn test() {
    let env = TestEnv::new().await;
    let riff_path = env.tmp_directory.join("disc.riff");
    fs::copy(
        env.fixture_source("Test Game (USA, Europe) (LD).riff"),
        &riff_path,
    )
    .await
    .unwrap();

    let riff = CommonRomfile::from_path(&riff_path)
        .unwrap()
        .as_riff()
        .await
        .unwrap();
    let first = env.tmp_directory.join("first");
    fs::create_dir(&first).await.unwrap();
    let chd = riff
        .to_chd(&env.progress_bar, &first, &[], &None, None)
        .await
        .unwrap();
    let chd = chd.romfile.as_chd().await.unwrap();
    assert!(chd.chd_type == ChdType::Ld);
    assert!(chd.size > 0);

    let extracted = env.tmp_directory.join("extracted");
    fs::create_dir(&extracted).await.unwrap();
    let riff = chd.to_riff(&env.progress_bar, &extracted).await.unwrap();

    let second = env.tmp_directory.join("second");
    fs::create_dir(&second).await.unwrap();
    let again = riff
        .to_chd(&env.progress_bar, &second, &[], &None, None)
        .await
        .unwrap();
    assert_eq!(
        fs::read(&chd.romfile.path).await.unwrap(),
        fs::read(&again.romfile.path).await.unwrap()
    );
}
