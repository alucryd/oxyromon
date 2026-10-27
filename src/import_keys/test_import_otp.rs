use super::*;
use tempfile::TempDir;

#[test]
fn test() {
    // given an OTP dump holding dummy common keys, and a keys file sharing one
    let directory = TempDir::new_in("tests").unwrap();
    let progress_bar = ProgressBar::hidden();
    let destination = directory.path().join(WIIU_KEYS_FILE);
    let keys = directory.path().join("keys.txt");
    std::fs::write(&keys, "22222222222222222222222222222222\n").unwrap();
    let otp = directory.path().join("otp.bin");
    let mut bytes = vec![0x80; 0x400];
    bytes[0xD0..0xE0].copy_from_slice(&[0x11; 16]);
    bytes[0xE0..0xF0].copy_from_slice(&[0x22; 16]);
    std::fs::write(&otp, &bytes).unwrap();

    // when
    import_keys(&progress_bar, &[&keys, &otp], &destination).unwrap();

    // then the Wii U common key is kept once, and the vWii's is added
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        "22222222222222222222222222222222\n\
         11111111111111111111111111111111\n"
    );
}
