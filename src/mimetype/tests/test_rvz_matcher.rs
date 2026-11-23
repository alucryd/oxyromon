use super::*;

#[test]
fn test_valid() {
    assert!(rvz_matcher(&[0x52, 0x56, 0x5A, 0x01]));
}

#[test]
fn test_invalid() {
    assert!(!rvz_matcher(&[0x00, 0x00, 0x00, 0x00]));
}

#[test]
fn test_too_short() {
    assert!(!rvz_matcher(&[0x52, 0x56, 0x5A]));
}
