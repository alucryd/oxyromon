use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    let env = TestEnv::new().await;
    // given a prod.keys whose keys are not the real ones
    let prod = env.tmp_directory.join("prod.keys");
    let keys = [
        "aes_kek_generation_source",
        "aes_key_generation_source",
        "titlekek_source",
        "key_area_key_application_source",
        "key_area_key_ocean_source",
        "key_area_key_system_source",
        "master_key_00",
    ]
    .map(|name| format!("{} = 00112233445566778899AABBCCDDEEFF\n", name))
    .concat();
    std::fs::write(&prod, keys).unwrap();

    // when
    let result = import_keys(&env.progress_bar, &[&prod], &env.tmp_directory);

    // then it is refused, and nothing is written
    assert!(format!("{:#}", result.unwrap_err()).contains("crc32 mismatch"));
    assert!(!env.tmp_directory.join(SWITCH_KEYS_FILE).exists());
}
