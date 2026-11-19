use super::*;
use tempfile::TempDir;

#[test]
fn test() {
    // given two of Azahar's aes_keys.txt, the second updating a key of the
    // first, with a comment, a blank line and an RSA modulus among them
    let directory = TempDir::new_in("tests").unwrap();
    let progress_bar = ProgressBar::hidden();
    let a = directory.path().join("aes_keys.txt");
    let b = directory.path().join("newer.txt");
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
    import_keys(&progress_bar, &[&a], directory.path()).unwrap();
    import_keys(&progress_bar, &[&b], directory.path()).unwrap();

    // then they are merged by name, the newer value winning, upper case, in
    // Azahar's format, and taken for neither Switch nor Wii U keys
    let destination = directory.path().join(N3DS_KEYS_FILE);
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
    assert!(!directory.path().join(SWITCH_KEYS_FILE).exists());
    assert!(!directory.path().join(WIIU_KEYS_FILE).exists());
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
