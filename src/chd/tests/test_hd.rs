use super::*;
use crate::test_support::TestEnv;
use tokio::fs;

/// A hard disk without an Amiga Rigid Disk Block, as most arcade disks are,
/// still extracts from its CHD: the `RDSK` magic only picks raw disks out on
/// input, it is not a property of every hard disk.
#[tokio::test]
async fn test() {
    let env = TestEnv::new().await;
    // An MBR-style first sector, then a pattern
    let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 251) as u8).collect();
    assert!(!data.starts_with(b"RDSK"));
    let rdsk_path = env.tmp_directory.join("disk.rdsk");
    fs::write(&rdsk_path, &data).await.unwrap();

    let rdsk = RdskRomfile {
        romfile: CommonRomfile::from_path(&rdsk_path).unwrap(),
    };
    let chd = rdsk
        .to_chd(&env.progress_bar, &env.tmp_directory, &[], &None, None)
        .await
        .unwrap();
    let chd = chd.romfile.as_chd().await.unwrap();
    assert!(chd.chd_type == ChdType::Hd);

    let extracted = env.tmp_directory.join("extracted");
    fs::create_dir(&extracted).await.unwrap();
    let rdsk = chd.to_rdsk(&env.progress_bar, &extracted).await.unwrap();
    assert_eq!(fs::read(&rdsk.romfile.path).await.unwrap(), data);
}
