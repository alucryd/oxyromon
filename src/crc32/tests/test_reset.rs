use digest::Digest;

use super::*;

#[test]
fn test() {
    let mut hasher = Crc32::new();
    Digest::update(&mut hasher, b"some data");
    Digest::reset(&mut hasher);
    Digest::update(&mut hasher, b"123456789");
    let result = hasher.finalize();
    assert_eq!(result.as_slice(), &[0xCB, 0xF4, 0x39, 0x26]);
}
