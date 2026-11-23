use digest::Digest;

use super::*;

#[test]
fn test() {
    // CRC32 of empty input is 0x00000000
    let result = Crc32::new().finalize();
    assert_eq!(result.as_slice(), &[0x00, 0x00, 0x00, 0x00]);
}
