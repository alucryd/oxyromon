use super::*;

#[test]
fn test_valid() {
    assert!(cso_matcher(&[0x43, 0x49, 0x53, 0x4F]));
}

#[test]
fn test_invalid() {
    assert!(!cso_matcher(&[0x00, 0x00, 0x00, 0x00]));
}

#[test]
fn test_too_short() {
    assert!(!cso_matcher(&[0x43, 0x49, 0x53]));
}
