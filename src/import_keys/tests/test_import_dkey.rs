use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    let env = TestEnv::new().await;
    // given two Redump .dkey files, one without a newline as Redump ships them
    let a = env.tmp_directory.join("Game A (USA).dkey");
    let b = env.tmp_directory.join("Game B (Europe).DKEY");
    std::fs::write(&a, "73819CEFE085B2C8FE8A92E7F81E84BC").unwrap();
    std::fs::write(&b, "be4b141d10725ef29a18b2e4f90f9952\r\n").unwrap();

    // when
    import_keys(&env.progress_bar, &[&a, &b], &env.tmp_directory).unwrap();

    // then both are kept, apart from the Wii U keys they look like
    let keys = load_ps3_keys(&env.tmp_directory.join(PS3_KEYS_FILE)).unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys[0][..2], [0x73, 0x81]);
    assert!(!env.tmp_directory.join(WIIU_KEYS_FILE).exists());

    // when imported again, alongside a bad one
    let bad = env.tmp_directory.join("Bad.dkey");
    std::fs::write(&bad, "73819CEF").unwrap();
    let result = import_keys(&env.progress_bar, &[&a, &bad], &env.tmp_directory);

    // then it is refused, and the keys are as they were
    assert!(result.is_err());
    assert_eq!(
        load_ps3_keys(&env.tmp_directory.join(PS3_KEYS_FILE)).unwrap(),
        keys
    );

    // and a key met again is kept once
    import_keys(&env.progress_bar, &[&a], &env.tmp_directory).unwrap();
    assert_eq!(
        load_ps3_keys(&env.tmp_directory.join(PS3_KEYS_FILE)).unwrap(),
        keys
    );
}
