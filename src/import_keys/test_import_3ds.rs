use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    let env = TestEnv::new().await;
    // given two of Azahar's aes_keys.txt, the second updating a key of the
    // first, with a comment, a blank line and an RSA modulus among them
    let a = env.tmp_directory.join("aes_keys.txt");
    let b = env.tmp_directory.join("newer.txt");
    std::fs::write(
        &a,
        "# Generator constant\n\
         generatorConstant=11111111111111111111111111111111\n\
         \n\
         slot0x2CKeyX=22222222222222222222222222222222\n\
         common0=33333333333333333333333333333333\n",
    )
    .unwrap();
    std::fs::write(
        &b,
        format!(
            "slot0x2CKeyX=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
             ticketWrapMod={}\n",
            "b".repeat(512)
        ),
    )
    .unwrap();

    // when
    import_keys(&env.progress_bar, &[&a], &env.tmp_directory).unwrap();
    import_keys(&env.progress_bar, &[&b], &env.tmp_directory).unwrap();

    // then they are merged by name, the newer value winning, upper case, in
    // Azahar's format, and taken for neither Switch nor Wii U keys
    let destination = env.tmp_directory.join(N3DS_KEYS_FILE);
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        format!(
            "generatorConstant=11111111111111111111111111111111\n\
             slot0x2CKeyX=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n\
             common0=33333333333333333333333333333333\n\
             ticketWrapMod={}\n",
            "B".repeat(512)
        )
    );
    assert!(!env.tmp_directory.join(SWITCH_KEYS_FILE).exists());
    assert!(!env.tmp_directory.join(WIIU_KEYS_FILE).exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&destination)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
