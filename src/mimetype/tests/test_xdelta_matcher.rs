use super::*;

#[test]
fn test_valid() {
    assert!(xdelta_matcher(&[0xD6, 0xC3, 0xC4]));
}

#[test]
fn test_invalid() {
    assert!(!xdelta_matcher(&[0x00, 0x00, 0x00]));
}

#[test]
fn test_too_short() {
    assert!(!xdelta_matcher(&[0xD6, 0xC3]));
}
