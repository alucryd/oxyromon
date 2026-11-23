use super::*;

#[test]
fn test_valid() {
    assert!(bps_matcher(&[0x42, 0x50, 0x53, 0x31]));
}

#[test]
fn test_invalid() {
    assert!(!bps_matcher(&[0x00, 0x00, 0x00, 0x00]));
}

#[test]
fn test_too_short() {
    assert!(!bps_matcher(&[0x42, 0x50, 0x53]));
}
