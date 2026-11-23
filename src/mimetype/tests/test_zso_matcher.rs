use super::*;

#[test]
fn test_valid() {
    assert!(zso_matcher(&[0x5A, 0x49, 0x53, 0x4F]));
}

#[test]
fn test_invalid() {
    assert!(!zso_matcher(&[0x00, 0x00, 0x00, 0x00]));
}

#[test]
fn test_too_short() {
    assert!(!zso_matcher(&[0x5A, 0x49, 0x53]));
}
