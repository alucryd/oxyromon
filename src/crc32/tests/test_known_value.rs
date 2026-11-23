use digest::Digest;

use super::*;

#[test]
fn test() {
    // CRC32 of "123456789" is 0xCBF43926
    let mut hasher = Crc32::new();
    Digest::update(&mut hasher, b"123456789");
    let result = hasher.finalize();
    assert_eq!(result.as_slice(), &[0xCB, 0xF4, 0x39, 0x26]);
}
