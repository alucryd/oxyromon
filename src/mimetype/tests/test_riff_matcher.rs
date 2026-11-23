use super::*;

#[test]
fn test_valid() {
    assert!(riff_matcher(&[0x52, 0x49, 0x46, 0x46]));
}

#[test]
fn test_invalid() {
    assert!(!riff_matcher(&[0x00, 0x00, 0x00, 0x00]));
}

#[test]
fn test_too_short() {
    assert!(!riff_matcher(&[0x52, 0x49, 0x46]));
}
