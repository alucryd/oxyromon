use super::*;
use crate::test_support::TestEnv;

#[tokio::test]
async fn test() {
    let env = TestEnv::new().await;
    // given a Wii U OTP dump holding dummy common keys, a BootMii keys.bin
    // holding a Wii OTP, and a keys file sharing one
    let destination = env.tmp_directory.join(WIIU_KEYS_FILE);
    let keys = env.tmp_directory.join("keys.txt");
    std::fs::write(&keys, "22222222222222222222222222222222\n").unwrap();
    let otp = env.tmp_directory.join("otp.bin");
    let mut bytes = vec![0x80; 0x400];
    bytes[0x14..0x24].copy_from_slice(&[0x33; 16]);
    bytes[0xD0..0xE0].copy_from_slice(&[0x11; 16]);
    bytes[0xE0..0xF0].copy_from_slice(&[0x22; 16]);
    std::fs::write(&otp, &bytes).unwrap();
    let bootmii = env.tmp_directory.join("keys.bin");
    let mut bytes = vec![0; 0x400];
    bytes[..9].copy_from_slice(b"BackupMii");
    bytes[0x114..0x124].copy_from_slice(&[0x44; 16]);
    std::fs::write(&bootmii, &bytes).unwrap();

    // when
    import_keys(
        &env.progress_bar,
        &[&keys, &otp, &bootmii],
        &env.tmp_directory,
    )
    .unwrap();

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
