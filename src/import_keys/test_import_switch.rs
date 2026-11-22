use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    let env = TestEnv::new().await;
    // given two title.keys files, one updating a key of the other, and a Cemu
    // keys.txt alongside
    let a = env.tmp_directory.join("title.keys");
    let b = env.tmp_directory.join("newer.keys");
    let cemu = env.tmp_directory.join("keys.txt");
    std::fs::write(
        &a,
        "01000000000000000000000000000000 = 11111111111111111111111111111111\n\
         02000000000000000000000000000000 = 22222222222222222222222222222222\n",
    )
    .unwrap();
    std::fs::write(
        &b,
        "02000000000000000000000000000000=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
         03000000000000000000000000000000 = 33333333333333333333333333333333\n",
    )
    .unwrap();
    std::fs::write(&cemu, "44444444444444444444444444444444\n").unwrap();

    // when
    import_keys(&env.progress_bar, &[&a], &env.tmp_directory).unwrap();
    import_keys(&env.progress_bar, &[&b, &cemu], &env.tmp_directory).unwrap();

    // then the Switch keys are merged by name, the newer value winning, and
    // kept apart from the Wii U keys
    let destination = env.tmp_directory.join(SWITCH_KEYS_FILE);
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        "01000000000000000000000000000000 = 11111111111111111111111111111111\n\
         02000000000000000000000000000000 = AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n\
         03000000000000000000000000000000 = 33333333333333333333333333333333\n"
    );
    assert_eq!(
        std::fs::read_to_string(env.tmp_directory.join(WIIU_KEYS_FILE)).unwrap(),
        "44444444444444444444444444444444\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&destination)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    // and nsz-rs reads them back as title keys
    let mut keys = nsz_rs::keys::Keys::default();
    keys.load_title_keys(&destination).unwrap();
    assert_eq!(
        keys.title_key_by_rights("02000000000000000000000000000000"),
        Some([0xAA; 16])
    );
}
