use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    let env = TestEnv::new().await;
    // given an aes_keys.txt whose slot key is a byte short
    let path = env.tmp_directory.join("aes_keys.txt");
    std::fs::write(
        &path,
        "generatorConstant=11111111111111111111111111111111\n\
         slot0x2CKeyX=222222222222222222222222222222\n",
    )
    .unwrap();

    // when
    let result = import_keys(&env.progress_bar, &[&path], &env.tmp_directory);

    // then it is refused, and nothing is written
    assert!(result.is_err());
    assert!(!env.tmp_directory.join(N3DS_KEYS_FILE).exists());
}
