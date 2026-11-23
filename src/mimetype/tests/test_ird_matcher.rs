use super::*;

#[test]
fn test_valid() {
    assert!(ird_matcher(&[0x33, 0x49, 0x52, 0x44]));
}

#[test]
fn test_invalid() {
    assert!(!ird_matcher(&[0x00, 0x00, 0x00, 0x00]));
}

#[test]
fn test_too_short() {
    assert!(!ird_matcher(&[0x33, 0x49, 0x52]));
}
