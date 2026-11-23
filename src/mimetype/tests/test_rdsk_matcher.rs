use super::*;

#[test]
fn test_valid() {
    assert!(rdsk_matcher(&[0x52, 0x44, 0x53, 0x4B]));
}

#[test]
fn test_invalid() {
    assert!(!rdsk_matcher(&[0x00, 0x00, 0x00, 0x00]));
}

#[test]
fn test_too_short() {
    assert!(!rdsk_matcher(&[0x52, 0x44, 0x53]));
}
