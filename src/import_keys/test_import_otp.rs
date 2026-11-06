use super::*;
use tempfile::TempDir;

#[test]
fn test() {
    // given a Wii U OTP dump holding dummy common keys, a BootMii keys.bin
    // holding a Wii OTP, and a keys file sharing one
    let directory = TempDir::new_in("tests").unwrap();
    let progress_bar = ProgressBar::hidden();
    let destination = directory.path().join(WIIU_KEYS_FILE);
    let keys = directory.path().join("keys.txt");
    std::fs::write(&keys, "22222222222222222222222222222222\n").unwrap();
    let otp = directory.path().join("otp.bin");
    let mut bytes = vec![0x80; 0x400];
    bytes[0x14..0x24].copy_from_slice(&[0x33; 16]);
    bytes[0xD0..0xE0].copy_from_slice(&[0x11; 16]);
    bytes[0xE0..0xF0].copy_from_slice(&[0x22; 16]);
    std::fs::write(&otp, &bytes).unwrap();
    let bootmii = directory.path().join("keys.bin");
    let mut bytes = vec![0; 0x400];
    bytes[..9].copy_from_slice(b"BackupMii");
    bytes[0x114..0x124].copy_from_slice(&[0x44; 16]);
    std::fs::write(&bootmii, &bytes).unwrap();

    // when
    import_keys(&progress_bar, &[&keys, &otp, &bootmii], &destination).unwrap();

    // then the Wii U common key is kept once, and the vWii's and the Wii's
    // are added
    assert_eq!(
        std::fs::read_to_string(&destination).unwrap(),
        "22222222222222222222222222222222\n\
         11111111111111111111111111111111\n\
         33333333333333333333333333333333\n\
         44444444444444444444444444444444\n"
    );
}
