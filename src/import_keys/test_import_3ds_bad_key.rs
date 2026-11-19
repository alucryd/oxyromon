use super::*;
use tempfile::TempDir;

#[test]
fn test() {
    // given an aes_keys.txt whose slot key is a byte short
    let directory = TempDir::new_in("tests").unwrap();
    let progress_bar = ProgressBar::hidden();
    let path = directory.path().join("aes_keys.txt");
    std::fs::write(
        &path,
        "generatorConstant=11111111111111111111111111111111\n\
         slot0x2CKeyX=222222222222222222222222222222\n",
    )
    .unwrap();

    // when
    let result = import_keys(&progress_bar, &[&path], directory.path());

    // then it is refused, and nothing is written
    assert!(result.is_err());
    assert!(!directory.path().join(N3DS_KEYS_FILE).exists());
}
