use digest::Digest;
use std::io::Write;

use super::*;

#[test]
fn test() {
    let mut hasher = Crc32::new();
    hasher.write_all(b"123456789").unwrap();
    hasher.flush().unwrap();
    let result = Digest::finalize(hasher);
    assert_eq!(result.as_slice(), &[0xCB, 0xF4, 0x39, 0x26]);
}
